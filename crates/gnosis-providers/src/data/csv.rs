use crate::util::{attrs, entity, ext_is, relationship};
use gnosis_core::{
    AnalysisResult, Confidence, ContentReader, Diagnostic, DiagnosticSeverity, KnowledgeRecord,
    ObjectDescriptor, ProtoData, ProviderId, Result, Support, UnderstandingProvider,
    UnderstandingStatus,
};

const MAX_SAMPLE_ROWS: usize = 5;

pub struct CsvProvider;

impl UnderstandingProvider for CsvProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("csv")
    }

    fn probe(&self, object: &ObjectDescriptor, _proto: &ProtoData) -> Support {
        if ext_is(&object.extension, &["csv", "tsv"]) {
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

        let delimiter = if object.extension.as_deref() == Some("tsv") {
            b'\t'
        } else {
            b','
        };

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .flexible(true)
            .from_reader(buf.as_slice());

        let headers: Vec<String> = match reader.headers() {
            Ok(h) => h.iter().map(|s| s.to_string()).collect(),
            Err(e) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("csv header error: {e}"),
                });
                Vec::new()
            }
        };

        let mut row_count = 0usize;
        let mut samples: Vec<Vec<String>> = Vec::new();
        let mut col_types: Vec<String> = headers.iter().map(|_| "unknown".into()).collect();

        for result in reader.records() {
            match result {
                Ok(record) => {
                    row_count += 1;
                    if samples.len() < MAX_SAMPLE_ROWS {
                        let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_types.len() {
                                col_types[i] = refine_type(&col_types[i], cell);
                            }
                        }
                        samples.push(row);
                    } else {
                        // Still count remaining rows without storing.
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!("csv row error: {e}"),
                    });
                }
            }
        }

        // If we only sampled, estimate is exact for full read since we iterate all.
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "dataset".into());

        let mut attr = attrs(&[
            ("format", "csv"),
            ("rows", &row_count.to_string()),
            ("columns", &headers.len().to_string()),
        ]);
        attr.insert("headers".into(), headers.join(","));

        let dataset = entity(
            "dataset",
            &name,
            path,
            &object.id,
            &provider,
            1,
            attr,
            Confidence::High,
        );

        let mut entities = vec![dataset.clone()];
        let mut relationships = Vec::new();

        for (i, header) in headers.iter().enumerate() {
            let ty = col_types.get(i).map(|s| s.as_str()).unwrap_or("unknown");
            let col = entity(
                "column",
                header,
                path,
                &object.id,
                &provider,
                1,
                attrs(&[("inferred_type", ty), ("index", &i.to_string())]),
                Confidence::Medium,
            );
            relationships.push(relationship(
                "has_column",
                &dataset.id,
                &col.id,
                &provider,
                path,
                1,
                Confidence::High,
            ));
            entities.push(col);
        }

        if !samples.is_empty() {
            let sample_preview = samples
                .iter()
                .map(|r| r.join("|"))
                .collect::<Vec<_>>()
                .join("; ");
            entities.push(entity(
                "sample",
                "preview",
                path,
                &object.id,
                &provider,
                1,
                {
                    let mut m = attrs(&[("rows", &samples.len().to_string())]);
                    m.insert("preview".into(), sample_preview.chars().take(400).collect());
                    m
                },
                Confidence::Medium,
            ));
        }

        let status = if headers.is_empty() && row_count == 0 {
            UnderstandingStatus::PartiallyUnderstood
        } else {
            UnderstandingStatus::Understood
        };

        Ok(AnalysisResult {
            record: KnowledgeRecord {
                entities,
                relationships,
                diagnostics,
                status: Some(status),
                classification_reason: Some("csv schema and sample extracted".into()),
            },
            status,
            classification_reason: Some("csv schema and sample extracted".into()),
        })
    }
}

fn refine_type(current: &str, cell: &str) -> String {
    let cell = cell.trim();
    if cell.is_empty() {
        return current.to_string();
    }
    let inferred = if cell.parse::<i64>().is_ok() {
        "integer"
    } else if cell.parse::<f64>().is_ok() {
        "float"
    } else if cell.eq_ignore_ascii_case("true") || cell.eq_ignore_ascii_case("false") {
        "boolean"
    } else {
        "string"
    };
    match (current, inferred) {
        ("unknown", t) => t.into(),
        (a, b) if a == b => a.to_string(),
        ("integer", "float") | ("float", "integer") => "float".into(),
        _ => "string".into(),
    }
}
