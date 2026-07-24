//! Sample Rust module for Gnosis fixtures.

use std::collections::HashMap;

pub struct Catalog {
    items: HashMap<String, f64>,
}

pub trait Priced {
    fn price(&self) -> f64;
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: String, price: f64) {
        self.items.insert(name, price);
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        self.items.get(name).copied()
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

pub fn summarize(cat: &Catalog) -> usize {
    cat.items.len()
}
