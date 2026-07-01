#![allow(dead_code)]

use crate::cli::erofs::tree::TreeNode;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

pub const EROFS_SUPER_OFFSET: u64 = 1024;
pub const EROFS_SUPER_MAGIC: u32 = 0xE0F5_E1E2;
pub const EROFS_BLKSIZ: u32 = 4096;
pub const EROFS_BLKSIZ_BITS: u8 = 12;
pub const EROFS_ISLOTBITS: u32 = 5;
pub const EROFS_ISLOT_SIZE: u32 = 32;
pub const EROFS_NULL_ADDR: u32 = 0xFFFF_FFFF;
pub const EROFS_FEATURE_COMPAT_SB_CHKSUM: u32 = 0x01;
pub const EROFS_INODE_EXTENDED_SIZE: u32 = 64;
pub const EROFS_XATTR_IBODY_HEADER_SIZE: u32 = 12;
pub const EROFS_DIRENT_SIZE: u32 = 12;
pub const EROFS_SUPERBLOCK_SIZE: u32 = 128;

pub const EROFS_FT_REG_FILE: u8 = 1;
pub const EROFS_FT_DIR: u8 = 2;
pub const EROFS_FT_CHRDEV: u8 = 3;
pub const EROFS_FT_BLKDEV: u8 = 4;
pub const EROFS_FT_FIFO: u8 = 5;
pub const EROFS_FT_SOCK: u8 = 6;
pub const EROFS_FT_SYMLINK: u8 = 7;

pub const S_IFMT: u16 = 0o170000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFLNK: u16 = 0o120000;
pub const S_IFCHR: u16 = 0o020000;
pub const S_IFBLK: u16 = 0o060000;
pub const S_IFIFO: u16 = 0o010000;
pub const S_IFSOCK: u16 = 0o140000;

pub const EROFS_XATTR_INDEX_USER: u8 = 1;
pub const EROFS_XATTR_INDEX_TRUSTED: u8 = 4;
pub const EROFS_XATTR_INDEX_SECURITY: u8 = 6;

pub const EROFS_INODE_FLAT_PLAIN: u8 = 0;
pub const EROFS_INODE_FLAT_INLINE: u8 = 2;
pub const EROFS_INODE_CHUNK_BASED: u8 = 4;

pub const EROFS_CHUNK_FORMAT_INDEXES: u16 = 0x0020;
pub const EROFS_FEATURE_INCOMPAT_CHUNKED_FILE: u32 = 0x04;
pub const EROFS_FEATURE_INCOMPAT_DEVICE_TABLE: u32 = 0x08;
pub const EROFS_DEVICE_SLOT_SIZE: u32 = 128;
pub const EROFS_CHUNK_INDEX_SIZE: u32 = 8;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

pub fn new_encode_dev(major: u32, minor: u32) -> u32 {
    (minor & 0xFF) | (major << 8) | ((minor & !0xFF) << 12)
}

pub fn erofs_xattr_align(size: usize) -> usize {
    (size + 3) & !3
}

pub fn dirent_file_type(node: &TreeNode) -> u8 {
    match node {
        TreeNode::RegularFile(_) => EROFS_FT_REG_FILE,
        TreeNode::Directory(_) => EROFS_FT_DIR,
        TreeNode::Symlink(_) => EROFS_FT_SYMLINK,
        TreeNode::CharDevice(_) => EROFS_FT_CHRDEV,
        TreeNode::BlockDevice(_) => EROFS_FT_BLKDEV,
        TreeNode::Fifo(_) => EROFS_FT_FIFO,
        TreeNode::Socket(_) => EROFS_FT_SOCK,
    }
}

pub fn mode_type_bits(node: &TreeNode) -> u16 {
    match node {
        TreeNode::RegularFile(_) => S_IFREG,
        TreeNode::Directory(_) => S_IFDIR,
        TreeNode::Symlink(_) => S_IFLNK,
        TreeNode::CharDevice(_) => S_IFCHR,
        TreeNode::BlockDevice(_) => S_IFBLK,
        TreeNode::Fifo(_) => S_IFIFO,
        TreeNode::Socket(_) => S_IFSOCK,
    }
}

pub fn xattr_prefix_index(name: &[u8]) -> Option<(u8, &[u8])> {
    if name.starts_with(b"user.") {
        Some((EROFS_XATTR_INDEX_USER, &name[5..]))
    } else if name.starts_with(b"trusted.") {
        Some((EROFS_XATTR_INDEX_TRUSTED, &name[8..]))
    } else if name.starts_with(b"security.") {
        Some((EROFS_XATTR_INDEX_SECURITY, &name[9..]))
    } else {
        None
    }
}
