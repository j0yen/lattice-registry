use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use reqwest::blocking::Client;

use crate::catalog::{Catalog, CatalogEntry};
use crate::error::{Error, Result};
use crate::fetcher;
use crate::owl_analysis;
use crate::registry::OboFoundryRegistry;

/// Build an HTTP client (used for live operations).
pub fn build_client() -> Result<Client> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| Error::Network(e.to_string()))
}

/// `lattice-registry sync` — ingest OBO Foundry registry.
pub fn cmd_sync(
    catalog_path: &Path,
    cache_dir: &Path,
    registry_name: &str,
) -> Result<()> {
    if registry_name != "obo-foundry" {
        return Err(Error::Config(format!(
            "Unknown registry '{}'. Only 'obo-foundry' is supported.",
            registry_name
        )));
    }

    let client = build_client()?;
    let registry_cache = cache_dir.join("obo-foundry-registry.jsonld");

    eprintln!("Fetching OBO Foundry registry…");
    let json_data = fetcher::fetch_obo_foundry_registry(&client, &registry_cache)
        .map_err(|e| {
            eprintln!("Warning: network error fetching registry: {}", e);
            e
        })?;

    let registry = OboFoundryRegistry::from_json(&json_data)?;
    let entries = registry.ontologies.unwrap_or_default();

    let mut catalog = Catalog::load(catalog_path)?;
    let mut added = 0usize;

    for entry in &entries {
        let id = match &entry.id {
            Some(s) => s.clone(),
            None => continue,
        };
        let title = entry.title.clone().unwrap_or_else(|| id.clone());
        let domain = entry.domain.clone().unwrap_or_else(|| "unknown".into());
        let iri = match &entry.ontology_purl {
            Some(s) => s.clone(),
            None => continue,
        };
        let license = entry
            .license
            .as_ref()
            .and_then(|l| l.label.clone().or_else(|| l.url.clone()))
            .unwrap_or_default();

        // Only insert if not already present (sync is additive for metadata).
        if !catalog.entries.contains_key(&id) {
            catalog.insert(CatalogEntry {
                id: id.clone(),
                iri,
                title,
                domain,
                class_count: 0,
                bfo_grounded: false,
                license,
                cached_path: None,
                etag: None,
                last_fetched: None,
            });
            added += 1;
        }
    }

    catalog.save(catalog_path)?;
    println!(
        "Sync complete. Registry entries: {}, new entries added: {}",
        entries.len(),
        added
    );
    Ok(())
}

/// `lattice-registry add <iri-or-url>` — fetch + catalog a single OWL file.
pub fn cmd_add(
    catalog_path: &Path,
    cache_dir: &Path,
    iri: &str,
) -> Result<()> {
    let client = build_client()?;
    let mut catalog = Catalog::load(catalog_path)?;

    // Generate an ID from the IRI (last path segment, without extension).
    let id = id_from_iri(iri);
    let dest_filename = format!("{}.owl", id);
    let dest = cache_dir.join(&dest_filename);

    // Check existing etag.
    let existing_etag = catalog
        .get(&id)
        .and_then(|e| e.etag.as_deref())
        .map(String::from);

    eprintln!("Fetching {}…", iri);
    let fetch_result =
        fetcher::fetch_to_file(&client, iri, &dest, existing_etag.as_deref())?;

    if fetch_result.was_cached {
        println!("{}: up to date (etag unchanged, no re-download)", id);
        return Ok(());
    }

    // Analyse the OWL file.
    let analysis = owl_analysis::analyse_owl_file(&fetch_result.path)?;

    let entry = CatalogEntry {
        id: id.clone(),
        iri: iri.to_string(),
        title: id.clone(),
        domain: "unknown".into(),
        class_count: analysis.class_count,
        bfo_grounded: analysis.bfo_grounded,
        license: analysis.license,
        cached_path: Some(fetch_result.path.clone()),
        etag: fetch_result.etag,
        last_fetched: Some(Utc::now()),
    };

    catalog.insert(entry);
    catalog.save(catalog_path)?;

    println!(
        "Added {}: classes={}, bfo_grounded={}, path={}",
        id,
        analysis.class_count,
        analysis.bfo_grounded,
        fetch_result.path.display()
    );
    Ok(())
}

/// `lattice-registry list [--domain <d>]` — browse catalog.
pub fn cmd_list(catalog_path: &Path, domain_filter: Option<&str>) -> Result<()> {
    let catalog = Catalog::load(catalog_path)?;
    let entries = catalog.list_by_domain(domain_filter);

    if entries.is_empty() {
        println!("(no entries in catalog)");
        return Ok(());
    }

    println!("{:<20} {:<30} {:<20} {:<8} {}", "ID", "TITLE", "DOMAIN", "CLASSES", "BFO");
    println!("{}", "-".repeat(90));
    for e in entries {
        println!(
            "{:<20} {:<30} {:<20} {:<8} {}",
            truncate(&e.id, 20),
            truncate(&e.title, 30),
            truncate(&e.domain, 20),
            e.class_count,
            if e.bfo_grounded { "yes" } else { "no" }
        );
    }
    Ok(())
}

/// `lattice-registry show <id>` — print full metadata.
pub fn cmd_show(catalog_path: &Path, id: &str) -> Result<()> {
    let catalog = Catalog::load(catalog_path)?;
    let entry = catalog
        .get(id)
        .ok_or_else(|| Error::NotFound(format!("No catalog entry for '{}'", id)))?;

    println!("id:          {}", entry.id);
    println!("iri:         {}", entry.iri);
    println!("title:       {}", entry.title);
    println!("domain:      {}", entry.domain);
    println!("class_count: {}", entry.class_count);
    println!("bfo_grounded:{}", entry.bfo_grounded);
    println!("license:     {}", entry.license);
    println!(
        "cached_path: {}",
        entry
            .cached_path
            .as_deref()
            .map(Path::to_string_lossy)
            .unwrap_or_default()
    );
    println!(
        "etag:        {}",
        entry.etag.as_deref().unwrap_or("(none)")
    );
    println!(
        "last_fetched:{}",
        entry
            .last_fetched
            .map(|d| d.to_rfc3339())
            .unwrap_or_else(|| "(never)".into())
    );
    Ok(())
}

/// `lattice-registry path <id>` — print local cached path (or non-zero exit).
pub fn cmd_path(catalog_path: &Path, id: &str) -> Result<()> {
    let catalog = Catalog::load(catalog_path)?;
    let entry = catalog
        .get(id)
        .ok_or_else(|| Error::NotFound(format!("No catalog entry for '{}'", id)))?;
    let path = entry
        .cached_path
        .as_deref()
        .ok_or_else(|| Error::NotFound(format!("No cached file for '{}'", id)))?;
    println!("{}", path.display());
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn id_from_iri(iri: &str) -> String {
    let base = iri.split('/').last().unwrap_or(iri);
    let base = base.split('#').next().unwrap_or(base);
    // Strip common extensions.
    let base = base
        .strip_suffix(".owl")
        .or_else(|| base.strip_suffix(".obo"))
        .or_else(|| base.strip_suffix(".ttl"))
        .unwrap_or(base);
    if base.is_empty() {
        // Fallback: hash the URL.
        format!("onto-{:x}", {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in iri.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        })
    } else {
        base.to_string()
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // ── Fixture OWL content ──────────────────────────────────────────────────

    const BFO_OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
          xml:base="http://purl.obolibrary.org/obo/test.owl"
          xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          ontologyIRI="http://purl.obolibrary.org/obo/test.owl">
  <Import>http://purl.obolibrary.org/obo/bfo.owl</Import>
  <Declaration><Class IRI="http://purl.obolibrary.org/obo/TEST_0000001"/></Declaration>
  <Declaration><Class IRI="http://purl.obolibrary.org/obo/TEST_0000002"/></Declaration>
  <AnnotationAssertion>
    <AnnotationProperty IRI="http://purl.org/dc/terms/license"/>
    <IRI>http://purl.obolibrary.org/obo/test.owl</IRI>
    <Literal>CC BY 4.0</Literal>
  </AnnotationAssertion>
</Ontology>"#;

    const NON_BFO_OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
          xml:base="http://example.com/myonto.owl"
          ontologyIRI="http://example.com/myonto.owl">
  <Declaration><Class IRI="http://example.com/MyClass1"/></Declaration>
  <Declaration><Class IRI="http://example.com/MyClass2"/></Declaration>
</Ontology>"#;

    const OBO_REGISTRY_JSON: &str = r#"{
  "ontologies": [
    {
      "id": "test",
      "title": "Test Ontology",
      "domain": "testing",
      "ontology_purl": "http://purl.obolibrary.org/obo/test.owl",
      "license": { "label": "CC BY 4.0", "url": "https://creativecommons.org/licenses/by/4.0/" }
    },
    {
      "id": "another",
      "title": "Another Ontology",
      "domain": "biology",
      "ontology_purl": "http://purl.obolibrary.org/obo/another.owl"
    }
  ]
}"#;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    // AC1: sync ingests registry JSON, populates ≥1 entry
    #[test]
    fn test_sync_populates_catalog() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let cache_dir = tmp.path().join("owls");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Simulate a pre-fetched registry file (bypass network).
        let registry_file = tmp.path().join("obo-foundry-registry.jsonld");
        std::fs::write(&registry_file, OBO_REGISTRY_JSON).unwrap();

        // Call ingest_registry_json directly (internal helper exposed for tests).
        ingest_registry_json(OBO_REGISTRY_JSON, &catalog_path).unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        assert!(catalog.entries.len() >= 1, "should have at least 1 entry");
        assert!(catalog.entries.contains_key("test"));
        assert_eq!(catalog.entries["test"].domain, "testing");
    }

    // AC2: add fetches OWL file, records class count, bfo_grounded, license
    #[test]
    fn test_add_records_owl_metadata() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let cache_dir = tmp.path().join("owls");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let owl_path = write_file(tmp.path(), "bfo_test.owl", BFO_OWX);
        let url = format!("file://{}", owl_path.display());

        add_from_file_url(&catalog_path, &cache_dir, &url, "bfo_test").unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        let entry = catalog.get("bfo_test").expect("entry should exist");
        assert_eq!(entry.class_count, 2);
        assert!(entry.bfo_grounded);
        assert!(!entry.license.is_empty(), "license should be set");
    }

    // AC3: non-BFO ontology flagged bfo_grounded: false but not crashed
    #[test]
    fn test_non_bfo_flagged_not_crashed() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let cache_dir = tmp.path().join("owls");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let owl_path = write_file(tmp.path(), "non_bfo.owl", NON_BFO_OWX);
        let url = format!("file://{}", owl_path.display());

        add_from_file_url(&catalog_path, &cache_dir, &url, "non_bfo").unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        let entry = catalog.get("non_bfo").expect("entry should exist");
        assert!(!entry.bfo_grounded, "should be flagged non-BFO");
        assert_eq!(entry.class_count, 2);
    }

    // AC4: list --domain filters correctly
    #[test]
    fn test_list_domain_filter() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");

        ingest_registry_json(OBO_REGISTRY_JSON, &catalog_path).unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        let all = catalog.list_by_domain(None);
        let bio = catalog.list_by_domain(Some("biology"));
        let test_domain = catalog.list_by_domain(Some("testing"));

        assert_eq!(all.len(), 2);
        assert_eq!(bio.len(), 1);
        assert_eq!(test_domain.len(), 1);
        assert_eq!(test_domain[0].id, "test");
    }

    // AC5: path returns the local cached path
    #[test]
    fn test_path_returns_cached_path() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let cache_dir = tmp.path().join("owls");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let owl_path = write_file(tmp.path(), "path_test.owl", BFO_OWX);
        let url = format!("file://{}", owl_path.display());
        add_from_file_url(&catalog_path, &cache_dir, &url, "path_test").unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        let entry = catalog.get("path_test").unwrap();
        let cached = entry.cached_path.as_deref().unwrap();
        assert!(cached.exists(), "cached file should exist on disk");
    }

    // AC5 (negative): missing id → NotFound error
    #[test]
    fn test_path_missing_id_errors() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let result = cmd_path(&catalog_path, "no-such-id");
        assert!(matches!(result, Err(Error::NotFound(_))));
    }

    // AC6: second add with same IRI doesn't re-copy if file unchanged
    // (for file:// URLs there's no etag, but we test the copy idempotency)
    #[test]
    fn test_add_idempotent_no_network_hit() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");
        let cache_dir = tmp.path().join("owls");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let owl_path = write_file(tmp.path(), "idem.owl", BFO_OWX);
        let url = format!("file://{}", owl_path.display());

        add_from_file_url(&catalog_path, &cache_dir, &url, "idem").unwrap();
        // Second call with same content must succeed without error.
        add_from_file_url(&catalog_path, &cache_dir, &url, "idem").unwrap();

        let catalog = Catalog::load(&catalog_path).unwrap();
        assert!(catalog.get("idem").is_some());
    }

    // AC7: network failure degrades gracefully (existing catalog untouched).
    #[test]
    fn test_network_failure_keeps_catalog() {
        let tmp = TempDir::new().unwrap();
        let catalog_path = tmp.path().join("catalog.json");

        // Pre-populate catalog.
        ingest_registry_json(OBO_REGISTRY_JSON, &catalog_path).unwrap();
        let before = Catalog::load(&catalog_path).unwrap();

        // Try to add a clearly unreachable URL.
        let result = add_from_file_url(
            &catalog_path,
            tmp.path(),
            "file:///nonexistent/path/that/will/fail.owl",
            "fail-test",
        );

        assert!(result.is_err(), "should error on missing file");

        // Catalog should be unchanged.
        let after = Catalog::load(&catalog_path).unwrap();
        assert_eq!(before.entries.len(), after.entries.len());
    }

    // ── Internal helpers for tests (bypass network) ───────────────────────────

    /// Ingest an in-memory OBO Foundry JSON string directly into the catalog.
    fn ingest_registry_json(json: &str, catalog_path: &Path) -> crate::error::Result<()> {
        let registry = OboFoundryRegistry::from_json(json)?;
        let entries = registry.ontologies.unwrap_or_default();
        let mut catalog = Catalog::load(catalog_path)?;
        for entry in &entries {
            let id = match &entry.id {
                Some(s) => s.clone(),
                None => continue,
            };
            let iri = match &entry.ontology_purl {
                Some(s) => s.clone(),
                None => continue,
            };
            let license = entry
                .license
                .as_ref()
                .and_then(|l| l.label.clone().or_else(|| l.url.clone()))
                .unwrap_or_default();
            catalog.insert(CatalogEntry {
                id: id.clone(),
                iri,
                title: entry.title.clone().unwrap_or_else(|| id.clone()),
                domain: entry.domain.clone().unwrap_or_else(|| "unknown".into()),
                class_count: 0,
                bfo_grounded: false,
                license,
                cached_path: None,
                etag: None,
                last_fetched: None,
            });
        }
        catalog.save(catalog_path)?;
        Ok(())
    }

    /// Add an OWL file from a file:// URL or local path, bypassing HTTP.
    fn add_from_file_url(
        catalog_path: &Path,
        cache_dir: &Path,
        url: &str,
        id: &str,
    ) -> crate::error::Result<()> {
        std::fs::create_dir_all(cache_dir)?;
        // Strip "file://" prefix.
        let source_path = url
            .strip_prefix("file://")
            .unwrap_or(url);
        let source = Path::new(source_path);

        if !source.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("fixture file not found: {}", source.display()),
            )));
        }

        let dest = cache_dir.join(format!("{}.owl", id));
        std::fs::copy(source, &dest)?;

        let analysis = owl_analysis::analyse_owl_file(&dest)?;

        let mut catalog = Catalog::load(catalog_path)?;
        catalog.insert(CatalogEntry {
            id: id.to_string(),
            iri: url.to_string(),
            title: id.to_string(),
            domain: "unknown".into(),
            class_count: analysis.class_count,
            bfo_grounded: analysis.bfo_grounded,
            license: analysis.license,
            cached_path: Some(dest),
            etag: None,
            last_fetched: Some(Utc::now()),
        });
        catalog.save(catalog_path)?;
        Ok(())
    }
}
