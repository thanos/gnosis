use thiserror::Error;

pub type Result<T> = std::result::Result<T, GnosisError>;

#[derive(Debug, Error)]
pub enum GnosisError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("provider error ({provider}): {message}")]
    Provider { provider: String, message: String },

    #[error("pipeline error: {0}")]
    Pipeline(String),

    #[error("export error: {0}")]
    Export(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("job error: {0}")]
    Job(String),

    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl GnosisError {
    pub fn provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
        }
    }
}
