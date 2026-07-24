use crate::{
    AttributeMap, Confidence, Entity, EntityId, Evidence, ObjectId, ProviderId, Relationship,
    RelationshipId, SourceSpan,
};
use std::collections::BTreeMap;
use std::path::Path;

pub fn evidence(
    provider: &ProviderId,
    summary: impl Into<String>,
    path: &Path,
    line: u32,
) -> Evidence {
    Evidence {
        summary: summary.into(),
        span: Some(SourceSpan::line(path.to_path_buf(), line)),
        provider: provider.clone(),
    }
}

pub fn entity(
    kind: &str,
    name: &str,
    path: &Path,
    source: &ObjectId,
    provider: &ProviderId,
    line: u32,
    attrs: AttributeMap,
    confidence: Confidence,
) -> Entity {
    let path_str = path.to_string_lossy();
    Entity {
        id: EntityId::generate(kind, name, &path_str),
        kind: kind.into(),
        name: name.into(),
        attributes: attrs,
        evidence: vec![evidence(provider, format!("{kind} {name}"), path, line)],
        confidence,
        source_object: source.clone(),
    }
}

pub fn relationship(
    kind: &str,
    from: &EntityId,
    to: &EntityId,
    provider: &ProviderId,
    path: &Path,
    line: u32,
    confidence: Confidence,
) -> Relationship {
    Relationship {
        id: RelationshipId::generate(kind, from.as_str(), to.as_str()),
        kind: kind.into(),
        from: from.clone(),
        to: to.clone(),
        attributes: BTreeMap::new(),
        evidence: vec![evidence(provider, kind, path, line)],
        confidence,
    }
}

pub fn attrs(pairs: &[(&str, &str)]) -> AttributeMap {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

pub fn line_of(source: &str, byte_offset: usize) -> u32 {
    let mut line = 1u32;
    for (i, b) in source.bytes().enumerate() {
        if i >= byte_offset {
            break;
        }
        if b == b'\n' {
            line += 1;
        }
    }
    line
}

pub fn ext_is(object_ext: &Option<String>, candidates: &[&str]) -> bool {
    object_ext
        .as_ref()
        .map(|e| candidates.iter().any(|c| e.eq_ignore_ascii_case(c)))
        .unwrap_or(false)
}
