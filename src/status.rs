use serde::{Deserialize, Serialize};

/// Final understanding classification for a source object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum UnderstandingStatus {
    Understood,
    PartiallyUnderstood,
    #[default]
    Unknown,
    Failed,
}

impl UnderstandingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Understood => "understood",
            Self::PartiallyUnderstood => "partial",
            Self::Unknown => "unknown",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for UnderstandingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
