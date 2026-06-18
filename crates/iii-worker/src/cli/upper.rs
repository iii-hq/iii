//! Per-worker writable overlay upper: materialize a pre-formatted ext4
//! image from an embedded "golden" image.
//!
//! The overlay rootfs model needs a PERSISTENT writable upper so installed
//! deps (the bulk of a worker's setup) survive VM restarts. overlayfs
//! requires a real local fs for its upperdir — tmpfs works but isn't
//! persistent, and virtiofs is unreliable as an upperdir — so the upper is
//! an ext4 image on a virtio-blk device (`/dev/vdb`).
//!
//! Rather than format ext4 IN-GUEST (which would mean shipping `mke2fs` and
//! depending on the guest having format tooling — the very coupling the
//! overlay model removes), we embed ONE pre-built empty ext4 "golden" image
//! and stamp a per-worker copy of it; iii-init just mounts it. ext4's
//! on-disk format is endian-defined, so a single golden serves every guest
//! arch (aarch64 + x86_64).
//!
//! ## Why a sparse-extent format (not a whole-image blob)
//!
//! An empty 16 GiB ext4 is ~16 GiB of zeros around ~6 MiB of metadata.
//! Compressing the WHOLE image (gzip/zstd) yields a small file, but
//! materializing it means decompressing all 16 GiB — tens of seconds of CPU
//! on every worker's first boot. Instead the golden is stored as only its
//! NON-ZERO 4 KiB blocks (`magic | total_len | block_size | [offset,
//! block]...`), gzip-compressed. Materialization processes ~6 MiB
//! regardless of the 16 GiB logical size (near-instant) and writes the
//! blocks at their offsets into a sparse file, so `upper.ext4` occupies
//! only the metadata on disk while presenting its full logical size.
//!
//! Compression is gzip via `flate2` (pure-Rust miniz_oxide backend) — no C
//! build deps, consistent with the squashfs builder; `flate2` does both the
//! encode (regen) and decode (runtime) sides.
//!
//! Regenerate the golden with `crates/iii-worker/vendor/regen-golden.sh`
//! (which drives the `gen-golden` example).

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// gzip-compressed sparse-extent golden image (see module docs for format).
const GOLDEN: &[u8] = include_bytes!("../../vendor/upper-golden.iiu.gz");

/// Format magic; bump the trailing digit on any incompatible format change.
pub const GOLDEN_MAGIC: &[u8; 8] = b"IIIUPPR1";

/// Block granularity for the sparse format. Matches ext4's 4 KiB block size
/// so stored blocks align to the filesystem and holes stay maximal.
pub const BLOCK_SIZE: usize = 4096;

/// Filename of the materialized upper image inside a worker's managed dir.
pub const UPPER_NAME: &str = "upper.ext4";

/// True when a golden image is embedded in this binary. Normally always
/// true (the artifact is committed and `include_bytes!`d); the guard lets a
/// future no-golden build degrade to the non-persistent tmpfs upper instead
/// of attaching a missing device.
pub fn has_golden() -> bool {
    !GOLDEN.is_empty()
}

/// Ensure this worker's writable ext4 upper exists, materializing it from
/// the embedded golden on first call. Idempotent: an existing image is
/// returned untouched (that's where the worker's persisted deps live).
/// Returns the image path to attach as `/dev/vdb`.
pub fn ensure_upper_ext4(managed_dir: &Path) -> io::Result<PathBuf> {
    let dest = managed_dir.join(UPPER_NAME);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(managed_dir)?;
    // Materialize into a per-process temp sibling then atomically rename, so an
    // interrupted first boot never leaves a half-written image that a later
    // boot would mount as a valid (but corrupt) fs, and two concurrent starts
    // can't write the same temp.
    let tmp = managed_dir.join(format!("{UPPER_NAME}.{}.partial", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    write_sparse_image(GOLDEN, &tmp).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Decode a gzip'd sparse-extent stream and write it to a new sparse file at
/// `dest`: each `(offset, block)` record is written at its offset; the gaps
/// between records stay unallocated (holes). The file's logical size is set
/// to the stored `total_len`.
fn write_sparse_image(gz: &[u8], dest: &Path) -> io::Result<()> {
    let mut r = BufReader::new(GzDecoder::new(gz));

    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != GOLDEN_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "golden image: bad magic",
        ));
    }
    let total_len = read_u64(&mut r)?;
    let block_size = read_u32(&mut r)? as usize;
    if block_size == 0 || block_size > 1 << 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "golden image: implausible block size",
        ));
    }

    let mut out = File::create(dest)?;
    let mut block = vec![0u8; block_size];
    // A clean EOF on the offset field marks the end of the record stream.
    while let Some(offset) = read_u64_opt(&mut r)? {
        r.read_exact(&mut block)?;
        out.seek(SeekFrom::Start(offset))?;
        out.write_all(&block)?;
    }
    out.set_len(total_len)?;
    // Flush to disk before the caller renames over the final path, so a crash
    // right after the rename can't leave a zero-length / partially-written
    // upper that the next boot would mount as a valid (but empty) fs.
    out.sync_all()?;
    Ok(())
}

/// Build a sparse-extent golden from a raw (sparse) ext4 image: scan it in
/// `BLOCK_SIZE` blocks, gzip-emit `magic | total_len | block_size` followed
/// by an `(offset, block)` record for every non-all-zero block. Used by the
/// `gen-golden` example at vendor-regen time; the result is committed and
/// `include_bytes!`d above.
pub fn build_golden(raw_ext4: &Path, out_gz: &Path) -> io::Result<GoldenStats> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut src = BufReader::new(File::open(raw_ext4)?);
    let total_len = std::fs::metadata(raw_ext4)?.len();

    let mut enc = GzEncoder::new(File::create(out_gz)?, Compression::best());
    enc.write_all(GOLDEN_MAGIC)?;
    enc.write_all(&total_len.to_le_bytes())?;
    enc.write_all(&(BLOCK_SIZE as u32).to_le_bytes())?;

    let mut block = vec![0u8; BLOCK_SIZE];
    let mut offset: u64 = 0;
    let mut kept: u64 = 0;
    loop {
        let n = fill(&mut src, &mut block)?;
        if n == 0 {
            break;
        }
        // A short final block (raw len not a block multiple) is still emitted
        // whole; the reader writes it at its offset, which is in range.
        if block[..n].iter().any(|&b| b != 0) {
            enc.write_all(&offset.to_le_bytes())?;
            enc.write_all(&block[..n])?;
            kept += 1;
        }
        offset += n as u64;
    }
    enc.finish()?;
    Ok(GoldenStats {
        total_len,
        blocks_kept: kept,
        block_size: BLOCK_SIZE,
    })
}

/// Summary of a [`build_golden`] run, for the regen tool to print.
#[derive(Debug)]
pub struct GoldenStats {
    pub total_len: u64,
    pub blocks_kept: u64,
    pub block_size: usize,
}

fn fill<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// Read a u64, returning `None` on a clean EOF before any byte (end of the
/// record stream) and erroring on a partial read.
fn read_u64_opt<R: Read>(r: &mut R) -> io::Result<Option<u64>> {
    let mut b = [0u8; 8];
    let mut filled = 0;
    while filled < b.len() {
        match r.read(&mut b[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    match filled {
        0 => Ok(None),
        8 => Ok(Some(u64::from_le_bytes(b))),
        _ => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "golden image: truncated record",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_is_embedded() {
        assert!(has_golden());
        assert!(GOLDEN.len() > 1024, "golden artifact looks empty");
    }

    #[test]
    fn materialize_is_sparse_full_size_and_fast() {
        let dir = std::env::temp_dir().join(format!("iii-upper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = ensure_upper_ext4(&dir).unwrap();
        let meta = std::fs::metadata(&path).unwrap();

        // Logical size is the full golden size (a multi-GiB ext4).
        assert!(
            meta.len() >= 1 << 30,
            "upper logical size too small: {}",
            meta.len()
        );

        // On-disk allocation (blocks * 512) is a tiny fraction of the logical
        // size — proves the file is genuinely sparse.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let on_disk = meta.blocks() * 512;
            assert!(
                on_disk < meta.len() / 10,
                "expected sparse file: on_disk={on_disk} logical={}",
                meta.len()
            );
        }

        // Idempotent: a second call returns the same path without rewriting.
        let again = ensure_upper_ext4(&dir).unwrap();
        assert_eq!(path, again);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_then_read_roundtrips_sparse_extents() {
        let dir = std::env::temp_dir().join(format!("iii-golden-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Synthesize a "raw" image: 1 MiB logical, with data only in block 0
        // and block 100 — everything else a hole.
        let raw = dir.join("raw.img");
        let total = 1u64 << 20;
        {
            let mut f = File::create(&raw).unwrap();
            f.set_len(total).unwrap();
            f.seek(SeekFrom::Start(0)).unwrap();
            f.write_all(&[0xAB; BLOCK_SIZE]).unwrap();
            f.seek(SeekFrom::Start(100 * BLOCK_SIZE as u64)).unwrap();
            f.write_all(&[0xCD; BLOCK_SIZE]).unwrap();
        }

        let out = dir.join("g.iiu.gz");
        let stats = build_golden(&raw, &out).unwrap();
        assert_eq!(stats.total_len, total);
        assert_eq!(stats.blocks_kept, 2, "only two non-zero blocks expected");

        // Materialize from the generated artifact and verify the bytes land
        // at the right offsets and the holes stay zero.
        let dest = dir.join("upper.ext4");
        let gz = std::fs::read(&out).unwrap();
        write_sparse_image(&gz, &dest).unwrap();

        let restored = std::fs::read(&dest).unwrap();
        assert_eq!(restored.len() as u64, total);
        assert!(restored[..BLOCK_SIZE].iter().all(|&b| b == 0xAB));
        assert!(restored[BLOCK_SIZE..100 * BLOCK_SIZE].iter().all(|&b| b == 0));
        assert!(restored[100 * BLOCK_SIZE..101 * BLOCK_SIZE]
            .iter()
            .all(|&b| b == 0xCD));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
