use thiserror::Error;

#[derive(Error, Debug)]
pub enum InitError {
    #[error("failed to create directory {path}: {source}")]
    Mkdir { path: String, source: nix::Error },

    #[error("failed to mount {target}: {source}")]
    Mount { target: String, source: nix::Error },

    #[error("failed to create symlink {path}: {source}")]
    Symlink {
        path: String,
        source: std::io::Error,
    },

    #[error("failed to set RLIMIT_NOFILE: {0}")]
    Rlimit(std::io::Error),

    #[error("failed to write {path}: {source}")]
    WriteFile {
        path: String,
        source: std::io::Error,
    },

    #[error("III_WORKER_CMD environment variable not set")]
    MissingWorkerCmd,

    #[error("failed to spawn worker process: {0}")]
    SpawnWorker(std::io::Error),

    #[error("failed to parse III_INIT_NOFILE value '{value}': {source}")]
    ParseNofile {
        value: String,
        source: std::num::ParseIntError,
    },

    #[error("failed to create network socket: {0}")]
    NetSocket(std::io::Error),

    #[error("failed to configure interface {iface}: {op} failed: {source}")]
    NetIoctl {
        iface: String,
        op: &'static str,
        source: std::io::Error,
    },

    #[error("failed to add default route: {0}")]
    NetRoute(std::io::Error),

    #[error("invalid IP address in {var}: {value}")]
    InvalidAddr { var: String, value: String },

    #[error("invalid CIDR prefix in III_INIT_CIDR: {0}")]
    InvalidCidr(String),
}
