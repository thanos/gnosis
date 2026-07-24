use crate::ids::{EntityId, ObjectId, ProviderId, RelationshipId};
use crate::status::UnderstandingStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub type AttributeMap = BTreeMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSpan {
    pub path: PathBuf,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl SourceSpan {
    pub fn line(path: PathBuf, line: u32) -> Self {
        Self {
            path,
            start_line: line,
            start_col: 1,
            end_line: line,
            end_col: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub summary: String,
    pub span: Option<SourceSpan>,
    pub provider: ProviderId,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
    Inferred,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Inferred => "inferred",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
    pub attributes: AttributeMap,
    pub evidence: Vec<Evidence>,
    pub confidence: Confidence,
    pub source_object: ObjectId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelationshipId,
    pub kind: String,
    pub from: EntityId,
    pub to: EntityId,
    pub attributes: AttributeMap,
    pub evidence: Vec<Evidence>,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KnowledgeRecord {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub diagnostics: Vec<Diagnostic>,
    pub status: Option<UnderstandingStatus>,
    pub classification_reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AnalysisResult {
    pub record: KnowledgeRecord,
    pub status: UnderstandingStatus,
    pub classification_reason: Option<String>,
}
