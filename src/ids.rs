use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn from_path(root: &Path, path: &Path) -> Self {
        let rel = path.strip_prefix(root).unwrap_or(path);
        Self(format!("obj:{}", rel.to_string_lossy().replace('\\', "/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn generate(kind: &str, name: &str, path: &str) -> Self {
        Self(format!("ent:{kind}:{name}@{path}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationshipId(String);

impl RelationshipId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn generate(kind: &str, from: &str, to: &str) -> Self {
        Self(format!("rel:{kind}:{from}->{to}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stable identity helper used when a path-based id is not enough.
pub fn random_suffix() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

#[derive(Clone, Debug)]
pub struct PathKey(pub PathBuf);

impl PathKey {
    pub fn new(p: impl Into<PathBuf>) -> Self {
        Self(p.into())
    }
}
