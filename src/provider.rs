use crate::connectors::types::{ObjectDescriptor, ProtoData};
use crate::content::ContentReader;
use crate::error::Result;
use crate::ids::ProviderId;
use crate::knowledge::AnalysisResult;

/// How strongly a provider claims it can handle an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    None = 0,
    Weak = 1,
    Partial = 2,
    Full = 3,
}

impl Support {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Weak => "weak",
            Self::Partial => "partial",
            Self::Full => "full",
        }
    }
}

pub trait UnderstandingProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn version(&self) -> &str {
        "0.1.0"
    }
    fn probe(&self, object: &ObjectDescriptor, proto: &ProtoData) -> Support;
    fn analyze(
        &self,
        object: &ObjectDescriptor,
        proto: &ProtoData,
        content: &mut dyn ContentReader,
    ) -> Result<AnalysisResult>;
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn UnderstandingProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn UnderstandingProvider>) {
        self.providers.push(provider);
    }

    pub fn providers(&self) -> &[Box<dyn UnderstandingProvider>] {
        &self.providers
    }

    /// Select the provider with the highest support level (registration order breaks ties).
    pub fn select(
        &self,
        object: &ObjectDescriptor,
        proto: &ProtoData,
    ) -> Option<(&dyn UnderstandingProvider, Support)> {
        let mut best: Option<(usize, Support)> = None;
        for (i, p) in self.providers.iter().enumerate() {
            let support = p.probe(object, proto);
            if support == Support::None {
                continue;
            }
            match best {
                None => best = Some((i, support)),
                Some((_, best_s)) if support > best_s => best = Some((i, support)),
                Some((best_i, best_s)) if support == best_s && i < best_i => {
                    best = Some((i, support))
                }
                _ => {}
            }
        }
        best.map(|(i, s)| (self.providers[i].as_ref(), s))
    }

    pub fn coverage_summary(&self) -> Vec<(String, String)> {
        self.providers
            .iter()
            .map(|p| (p.id().to_string(), p.version().to_string()))
            .collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
