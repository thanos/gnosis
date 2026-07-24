use gnosis::OkfExporter;
use gnosis::{
    Confidence, Entity, EntityId, Exporter, KnowledgeStore, ObjectDescriptor, ObjectId, ProtoData,
    ProviderId, Relationship, RelationshipId, StoredObject, UnderstandingStatus,
};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn okf_export_writes_bundle_layout() {
    let mut store = KnowledgeStore::new();
    store.set_root(PathBuf::from("/repo"));
    store.set_enabled_providers(vec!["test".into()]);

    let obj_id = ObjectId::new("obj:a.rs");
    let ent = Entity {
        id: EntityId::new("ent:fn:foo"),
        kind: "function".into(),
        name: "foo".into(),
        attributes: [("lang".into(), "rust".into())].into_iter().collect(),
        evidence: Vec::new(),
        confidence: Confidence::High,
        source_object: obj_id.clone(),
    };
    let ent2 = Entity {
        id: EntityId::new("ent:struct:Bar"),
        kind: "struct".into(),
        name: "Bar".into(),
        attributes: Default::default(),
        evidence: Vec::new(),
        confidence: Confidence::Medium,
        source_object: obj_id.clone(),
    };
    store.upsert_object(StoredObject {
        descriptor: ObjectDescriptor {
            id: obj_id.clone(),
            path: PathBuf::from("/repo/a.rs"),
            relative_path: PathBuf::from("a.rs"),
            is_dir: false,
            size: 1,
            modified: None,
            media_type: "text/x-rust".into(),
            extension: Some("rs".into()),
        },
        proto: ProtoData::default(),
        status: UnderstandingStatus::Understood,
        provider: Some(ProviderId::new("test")),
        classification_reason: Some("ok".into()),
        entity_ids: vec![ent.id.clone(), ent2.id.clone()],
        diagnostics: Vec::new(),
    });
    store.add_entity(ent.clone());
    store.add_entity(ent2.clone());
    store.add_relationship(Relationship {
        id: RelationshipId::generate("defines", ent.id.as_str(), ent2.id.as_str()),
        kind: "defines".into(),
        from: ent.id,
        to: ent2.id,
        attributes: Default::default(),
        evidence: Vec::new(),
        confidence: Confidence::High,
    });

    let dir = TempDir::new().unwrap();
    let out = dir.path().join("knowledge.okf");
    OkfExporter::new().export(&store, &out).unwrap();

    assert!(out.join("index.md").exists());
    assert!(out.join("sidecar.json").exists());
    assert!(out.join("log.md").exists());
    assert!(out.join("entities").is_dir());
    assert!(out.join("objects").is_dir());
    assert!(out.join("relationships").is_dir());
    assert!(std::fs::read_dir(out.join("entities")).unwrap().count() >= 2);
    assert_eq!(OkfExporter::new().name(), "okf");
}
