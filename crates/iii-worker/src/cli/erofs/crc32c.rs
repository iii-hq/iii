//! CRC32C (Castagnoli) implementation shared by EROFS and ext4 formatters.

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Raw CRC32C with configurable seed and no final XOR.
///
/// This is the primitive used by ext4 metadata checksums (which chain
/// multiple CRC calls with intermediate seeds). For standard CRC32C,
/// call with `seed = 0xFFFF_FFFF` and XOR the result with `0xFFFF_FFFF`.
pub(crate) fn crc32c_raw(seed: u32, data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for i in 0..256u32 {
            let mut crc = i;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0x82F6_3B78;
                } else {
                    crc >>= 1;
                }
            }
            t[i as usize] = crc;
        }
        t
    });

    let mut crc = seed;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ table[index];
    }
    crc
}
