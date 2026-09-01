use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid or unsupported link")]
    InvalidUrl,
    #[error("Required executable was not found: {0}")]
    MissingDependency(String),
    #[error("Could not start {0}: {1}")]
    ProcessStart(String, String),
    #[error("{0} failed: {1}")]
    CommandFailed(String, String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("URL error: {0}")]
    Url(#[from] url::ParseError),
}

pub type AppResult<T> = Result<T, AppError>;
