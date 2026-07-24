use crate::store::KnowledgeStore;
use std::path::Path;

/// Export boundary — knowledge model is independent of serialization format.
pub trait Exporter: Send + Sync {
    fn name(&self) -> &str;
    fn export(&self, store: &KnowledgeStore, output: &Path) -> crate::error::Result<()>;
}
