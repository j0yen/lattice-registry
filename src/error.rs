use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("OWL parse error: {0}")]
    OwlParse(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Network failure: {0}")]
    Network(String),
}
