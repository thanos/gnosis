use crate::util::{attrs, entity, ext_is};
use gnosis_core::{
    AnalysisResult, Confidence, ContentReader, Diagnostic, DiagnosticSeverity, KnowledgeRecord,
    ObjectDescriptor, ProtoData, ProviderId, Result, Support, UnderstandingProvider,
    UnderstandingStatus,
};

const MAX_KEYS: usize = 64;

pub struct TomlProvider;

impl UnderstandingProvider for TomlProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("toml")
    }

    fn probe(&self, object: &ObjectDescriptor, _proto: &ProtoData) -> Support {
        if ext_is(&object.extension, &["toml"]) {
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
        let text = String::from_utf8_lossy(&buf);
        let provider = self.id();
        let path = &object.relative_path;
        let mut diagnostics = Vec::new();

        let value: toml::Value = match text.parse() {
            Ok(v) => v,
            Err(e) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("toml parse error: {e}"),
                });
                let doc = entity(
                    "document",
                    &path.display().to_string(),
                    path,
                    &object.id,
                    &provider,
                    1,
                    attrs(&[("format", "toml"), ("valid", "false")]),
                    Confidence::Low,
                );
                return Ok(AnalysisResult {
                    record: KnowledgeRecord {
                        entities: vec![doc],
                        relationships: Vec::new(),
                        diagnostics,
                        status: Some(UnderstandingStatus::Failed),
                        classification_reason: Some("toml parse failed".into()),
                    },
                    status: UnderstandingStatus::Failed,
                    classification_reason: Some("toml parse failed".into()),
                });
            }
        };

        let mut keys = Vec::new();
        collect_toml_keys(&value, "", 0, &mut keys);

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "toml".into());

        let mut attr = attrs(&[("format", "toml"), ("key_count", &keys.len().to_string())]);
        if let toml::Value::Table(table) = &value {
            let top: Vec<_> = table.keys().take(32).cloned().collect();
            attr.insert("top_level_keys".into(), top.join(","));
            if table.contains_key("package") || table.contains_key("dependencies") {
                attr.insert("manifest".into(), "cargo".into());
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
                classification_reason: Some("toml structure extracted".into()),
            },
            status: UnderstandingStatus::Understood,
            classification_reason: Some("toml structure extracted".into()),
        })
    }
}

fn collect_toml_keys(value: &toml::Value, prefix: &str, depth: usize, out: &mut Vec<String>) {
    if depth > 4 || out.len() >= MAX_KEYS {
        return;
    }
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.push(path.clone());
                collect_toml_keys(v, &path, depth + 1, out);
            }
        }
        toml::Value::Array(arr) => {
            if let Some(first) = arr.first() {
                let path = format!("{prefix}[]");
                out.push(path.clone());
                collect_toml_keys(first, &path, depth + 1, out);
            }
        }
        _ => {}
    }
}
