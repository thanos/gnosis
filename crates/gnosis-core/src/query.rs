use crate::ids::EntityId;
use crate::status::UnderstandingStatus;
use crate::store::{FindResults, GraphNeighborhood, InventoryCounts, KnowledgeStore, StoredObject};
use crate::knowledge::Entity;

pub struct QueryEngine<'a> {
    store: &'a KnowledgeStore,
}

impl<'a> QueryEngine<'a> {
    pub fn new(store: &'a KnowledgeStore) -> Self {
        Self { store }
    }

    pub fn summary(&self) -> String {
        let inv = self.store.inventory();
        let root = self
            .store
            .root()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".into());
        let branch = self
            .store
            .git_branch()
            .map(|b| format!("branch: {b}"))
            .unwrap_or_else(|| "not a git repo".into());
        format!(
            "Gnosis summary\n\
             root: {root}\n\
             {branch}\n\
             objects: {}\n\
             understood: {}  partial: {}  unknown: {}  failed: {}\n\
             modules: {}  types: {}  functions: {}\n\
             documents: {}  datasets: {}\n\
             relationships: {}\n\
             providers: {}\n",
            inv.source_objects,
            inv.understood,
            inv.partial,
            inv.unknown,
            inv.failed,
            inv.modules,
            inv.types,
            inv.functions,
            inv.documents,
            inv.datasets,
            inv.relationships,
            self.store.enabled_providers().join(", ")
        )
    }

    pub fn objects(
        &self,
        status: Option<UnderstandingStatus>,
        kind_substr: Option<&str>,
    ) -> Vec<&'a StoredObject> {
        let mut objs: Vec<_> = self.store.objects().collect();
        if let Some(status) = status {
            objs.retain(|o| o.status == status);
        }
        if let Some(k) = kind_substr {
            let k = k.to_lowercase();
            objs.retain(|o| {
                o.descriptor.media_type.to_lowercase().contains(&k)
                    || o.descriptor
                        .extension
                        .as_ref()
                        .map(|e| e.to_lowercase().contains(&k))
                        .unwrap_or(false)
            });
        }
        objs.sort_by(|a, b| {
            a.descriptor
                .relative_path
                .cmp(&b.descriptor.relative_path)
        });
        objs
    }

    pub fn unknown(&self) -> Vec<&'a StoredObject> {
        let mut objs: Vec<_> = self
            .store
            .objects()
            .filter(|o| {
                matches!(
                    o.status,
                    UnderstandingStatus::Unknown | UnderstandingStatus::PartiallyUnderstood
                )
            })
            .collect();
        objs.sort_by(|a, b| {
            a.descriptor
                .relative_path
                .cmp(&b.descriptor.relative_path)
        });
        objs
    }

    pub fn providers(&self) -> Vec<String> {
        self.store.enabled_providers().to_vec()
    }

    pub fn stats(&self) -> InventoryCounts {
        self.store.inventory()
    }

    pub fn find(&self, text: &str) -> FindResults {
        self.store.find(text)
    }

    pub fn explain(&self, query: &str) -> Option<ExplainResult<'a>> {
        // Try entity by id or name, then object by path/id.
        if let Some(entity) = self.store.get_entity(&EntityId::new(query)) {
            let neighborhood = self.store.neighborhood(&entity.id, 1);
            return Some(ExplainResult::Entity {
                entity,
                neighborhood,
            });
        }
        let by_name = self.store.find_entities_by_name(query);
        if let Some(entity) = by_name.first() {
            let neighborhood = self.store.neighborhood(&entity.id, 1);
            // Re-borrow from store
            if let Some(entity) = self.store.get_entity(&by_name[0].id) {
                return Some(ExplainResult::Entity {
                    entity,
                    neighborhood,
                });
            }
        }
        if let Some(obj) = self.store.get_object_by_path(query) {
            return Some(ExplainResult::Object { object: obj });
        }
        for obj in self.store.objects() {
            if obj.descriptor.id.as_str() == query
                || obj
                    .descriptor
                    .relative_path
                    .to_string_lossy()
                    .ends_with(query)
            {
                return Some(ExplainResult::Object { object: obj });
            }
        }
        None
    }

    pub fn graph(&self, query: &str, depth: usize) -> Option<(Entity, GraphNeighborhood)> {
        let entity = if let Some(e) = self.store.get_entity(&EntityId::new(query)) {
            e.clone()
        } else {
            self.store
                .find_entities_by_name(query)
                .into_iter()
                .next()?
                .clone()
        };
        let neighborhood = self.store.neighborhood(&entity.id, depth);
        Some((entity, neighborhood))
    }
}

pub enum ExplainResult<'a> {
    Entity {
        entity: &'a Entity,
        neighborhood: GraphNeighborhood,
    },
    Object {
        object: &'a StoredObject,
    },
}
