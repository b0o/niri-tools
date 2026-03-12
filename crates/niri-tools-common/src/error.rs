#[derive(Debug, thiserror::Error)]
pub enum NiriToolsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Niri command failed: {0}")]
    NiriCommand(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, NiriToolsError>;
