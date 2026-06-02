use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("upstream error: {0}")]
    Upstream(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("trace error: {0}")]
    Trace(String),
}
