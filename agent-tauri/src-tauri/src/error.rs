use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AgentError {
    #[error("{0}")]
    Relay(String),
    #[error("{0}")]
    Lcu(String),
    #[error("{0}")]
    Session(String),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Update(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub(crate) type AgentResult<T> = std::result::Result<T, AgentError>;
