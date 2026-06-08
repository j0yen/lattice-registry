use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// A single entry in the lattice ontology catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub iri: String,
    pub title: String,
    pub domain: String,
    pub class_count: u64,
    pub bfo_grounded: bool,
    pub license: String,
    pub cached_path: Option<PathBuf>,
    pub etag: Option<String>,
    pub last_fetched: Option<DateTime<Utc>>,
}

/// The on-disk catalog: a map from id → CatalogEntry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub entries: HashMap<String, CatalogEntry>,
}

impl Catalog {
    /// Load from disk (returns empty catalog if file absent).
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(path)?;
        let catalog = serde_json::from_str(&data)?;
        Ok(catalog)
    }

    /// Persist to disk, creating parent dirs as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn insert(&mut self, entry: CatalogEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    pub fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.get(id)
    }

    pub fn list_by_domain(&self, domain: Option<&str>) -> Vec<&CatalogEntry> {
        let mut entries: Vec<&CatalogEntry> = self
            .entries
            .values()
            .filter(|e| match domain {
                Some(d) => e.domain.to_lowercase().contains(&d.to_lowercase()),
                None => true,
            })
            .collect();
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        entries
    }
}

/// Default catalog storage path.
pub fn default_catalog_path() -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| Error::Config("cannot determine cache dir".into()))?;
    Ok(cache_dir.join("lattice").join("registry").join("catalog.json"))
}

/// Default directory for cached OWL files.
pub fn default_cache_dir() -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| Error::Config("cannot determine cache dir".into()))?;
    Ok(cache_dir.join("lattice").join("registry").join("owls"))
}
