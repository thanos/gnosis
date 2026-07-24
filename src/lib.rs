//! Gnosis — enterprise knowledge compiler.
//!
//! Library API for scanning repositories, extracting structured knowledge, and
//! exporting OKF-style bundles. The `gnosis` binary is a thin CLI over this crate.

pub mod config;
pub mod connectors;
pub mod content;
pub mod error;
pub mod events;
pub mod exporter;
pub mod ids;
pub mod knowledge;
pub mod okf;
pub mod pipeline;
pub mod provider;
pub mod providers;
pub mod query;
pub mod status;
pub mod store;
pub mod tui;

pub use config::{ScanConfig, ScanMetrics};
pub use connectors::{FilesystemConnector, GitContext, GitProtoData, ObjectDescriptor, ProtoData};
pub use content::{BytesContentReader, ContentReader, LimitedReader};
pub use error::{GnosisError, Result};
pub use events::PipelineEvent;
pub use exporter::Exporter;
pub use ids::{EntityId, ObjectId, ProviderId, RelationshipId};
pub use knowledge::{
    AnalysisResult, AttributeMap, Confidence, Diagnostic, DiagnosticSeverity, Entity, Evidence,
    KnowledgeRecord, Relationship, SourceSpan,
};
pub use okf::OkfExporter;
pub use pipeline::{Pipeline, PipelineHandle};
pub use provider::{ProviderRegistry, Support, UnderstandingProvider};
pub use providers::{
    default_registry, CppProvider, CsvProvider, ElixirProvider, GenericMetadataProvider,
    JsonProvider, MarkdownProvider, PlainTextProvider, RustProvider, TomlProvider, YamlProvider,
};
pub use query::{ExplainResult, QueryEngine};
pub use status::UnderstandingStatus;
pub use store::{FindResults, GraphNeighborhood, InventoryCounts, KnowledgeStore, StoredObject};
pub use tui::{drain_events_headless, TuiApp};
