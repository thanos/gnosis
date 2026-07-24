use crate::providers::util::{attrs, entity, ext_is};
use crate::{
    AnalysisResult, Confidence, ContentReader, Diagnostic, DiagnosticSeverity, KnowledgeRecord,
    ObjectDescriptor, ProtoData, ProviderId, Result, Support, UnderstandingProvider,
    UnderstandingStatus,
};

const MAX_KEYS: usize = 64;
const MAX_DEPTH: usize = 4;

pub struct JsonProvider;

impl UnderstandingProvider for JsonProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("json")
    }

    fn probe(&self, object: &ObjectDescriptor, _proto: &ProtoData) -> Support {
        if ext_is(&object.extension, &["json"]) {
            Support::Full
        } else {
            Support::None
        }
    }

    fn analyze(
        &self,
        object: &ObjectDescriptor,
        _proto: &ProtoData,
        content: &mut dyn ContentReader,
    ) -> Result<AnalysisResult> {
        let mut buf = Vec::new();
        content.read_to_end(&mut buf)?;
        let provider = self.id();
        let path = &object.relative_path;
        let mut diagnostics = Vec::new();

        let value: serde_json::Value = match serde_json::from_slice(&buf) {
            Ok(v) => v,
            Err(e) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("json parse error: {e}"),
                });
                let doc = entity(
                    "document",
                    &path.display().to_string(),
                    path,
                    &object.id,
                    &provider,
                    1,
                    attrs(&[("format", "json"), ("valid", "false")]),
                    Confidence::Low,
                );
                return Ok(AnalysisResult {
                    record: KnowledgeRecord {
                        entities: vec![doc],
                        relationships: Vec::new(),
                        diagnostics,
                        status: Some(UnderstandingStatus::Failed),
                        classification_reason: Some("json parse failed".into()),
                    },
                    status: UnderstandingStatus::Failed,
                    classification_reason: Some("json parse failed".into()),
                });
            }
        };

        let mut keys = Vec::new();
        collect_keys(&value, "", 0, &mut keys);

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "json".into());

        let mut attr = attrs(&[("format", "json"), ("key_count", &keys.len().to_string())]);
        if let Some(obj) = value.as_object() {
            let top: Vec<_> = obj.keys().take(32).cloned().collect();
            attr.insert("top_level_keys".into(), top.join(","));
            // Common manifests
            if obj.contains_key("dependencies") || obj.contains_key("devDependencies") {
                attr.insert("manifest".into(), "npm-like".into());
            }
            if obj.contains_key("packages") && obj.contains_key("metadata") {
                attr.insert("manifest".into(), "cargo-lock-like".into());
            }
        }

        let mut entities = vec![entity(
            "document",
            &name,
            path,
            &object.id,
            &provider,
            1,
            attr,
            Confidence::High,
        )];

        for key in keys.into_iter().take(MAX_KEYS) {
            entities.push(entity(
                "key_path",
                &key,
                path,
                &object.id,
                &provider,
                1,
                attrs(&[("path", &key)]),
                Confidence::Medium,
            ));
        }

        Ok(AnalysisResult {
            record: KnowledgeRecord {
                entities,
                relationships: Vec::new(),
                diagnostics,
                status: Some(UnderstandingStatus::Understood),
                classification_reason: Some("json structure extracted".into()),
            },
            status: UnderstandingStatus::Understood,
            classification_reason: Some("json structure extracted".into()),
        })
    }
}

fn collect_keys(value: &serde_json::Value, prefix: &str, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH || out.len() >= MAX_KEYS {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.push(path.clone());
                collect_keys(v, &path, depth + 1, out);
                if out.len() >= MAX_KEYS {
                    return;
                }
            }
        }
        serde_json::Value::Array(arr) => {
            if let Some(first) = arr.first() {
                let path = format!("{prefix}[]");
                out.push(path.clone());
                collect_keys(first, &path, depth + 1, out);
            }
        }
        _ => {}
    }
}
