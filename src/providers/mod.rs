//! Understanding providers for Gnosis.
//!
//! Deterministic Tree-sitter and lightweight document/data parsers that emit
//! structured knowledge records for the Gnosis pipeline.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::manual_strip)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::single_match)]

mod code;
mod data;
mod docs;
mod generic;
mod util;

pub use code::{CppProvider, ElixirProvider, RustProvider};
pub use data::{CsvProvider, JsonProvider, TomlProvider, YamlProvider};
pub use docs::{MarkdownProvider, PlainTextProvider};
pub use generic::GenericMetadataProvider;

use crate::ProviderRegistry;

/// Build the default deterministic provider set in registration priority order.
pub fn default_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(CppProvider));
    registry.register(Box::new(RustProvider));
    registry.register(Box::new(ElixirProvider));
    registry.register(Box::new(MarkdownProvider));
    registry.register(Box::new(PlainTextProvider));
    registry.register(Box::new(JsonProvider));
    registry.register(Box::new(YamlProvider));
    registry.register(Box::new(TomlProvider));
    registry.register(Box::new(CsvProvider));
    // Weak fallback — always last among competing Full/Partial peers for unsupported types.
    registry.register(Box::new(GenericMetadataProvider));
    registry
}
