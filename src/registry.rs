use serde::Deserialize;

/// A single entry from the OBO Foundry JSON-LD registry.
#[derive(Debug, Clone, Deserialize)]
pub struct OboFoundryEntry {
    pub id: Option<String>,
    pub title: Option<String>,
    pub domain: Option<String>,
    #[serde(rename = "ontology_purl")]
    pub ontology_purl: Option<String>,
    pub license: Option<OboLicense>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OboLicense {
    pub label: Option<String>,
    pub url: Option<String>,
}

/// Top-level OBO Foundry JSON-LD document.
#[derive(Debug, Deserialize)]
pub struct OboFoundryRegistry {
    pub ontologies: Option<Vec<OboFoundryEntry>>,
}

impl OboFoundryRegistry {
    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}
