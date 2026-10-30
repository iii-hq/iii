use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use devices::virtio::{
    block::{ImageType, SyncMode},
    Block, CacheType,
};
use utils::metrics::MetricsWriter;

#[derive(Debug)]
pub enum BlockConfigError {
    /// Failed to create the block device.
    CreateBlockDevice(std::io::Error),
}

impl fmt::Display for BlockConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::BlockConfigError::*;
        match *self {
            CreateBlockDevice(ref e) => write!(f, "Cannot create block device: {e:?}"),
        }
    }
}

type Result<T> = std::result::Result<T, BlockConfigError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDeviceConfig {
    pub block_id: String,
    pub cache_type: CacheType,
    pub disk_image_path: String,
    pub disk_image_format: ImageType,
    pub is_disk_read_only: bool,
    pub direct_io: bool,
    pub sync_mode: SyncMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockRootConfig {
    pub device: String,
    pub fstype: Option<String>,
    pub options: Option<String>,
}

#[derive(Default)]
pub struct BlockBuilder {
    pub list: VecDeque<Arc<Mutex<Block>>>,
}

impl BlockBuilder {
    pub fn new() -> Self {
        Self {
            list: VecDeque::<Arc<Mutex<Block>>>::new(),
        }
    }

    pub fn insert(&mut self, config: BlockDeviceConfig, metrics: MetricsWriter) -> Result<()> {
        let block_dev = Arc::new(Mutex::new(Self::create_block(config, metrics)?));
        self.list.push_back(block_dev);
        Ok(())
    }

    pub fn create_block(config: BlockDeviceConfig, metrics: MetricsWriter) -> Result<Block> {
        let device_metrics = metrics.register_block_device(config.block_id.clone());
        devices::virtio::Block::new(
            config.block_id,
            None,
            config.cache_type,
            config.disk_image_path,
            config.disk_image_format,
            config.is_disk_read_only,
            config.direct_io,
            config.sync_mode,
            device_metrics,
        )
        .map_err(BlockConfigError::CreateBlockDevice)
    }
}
