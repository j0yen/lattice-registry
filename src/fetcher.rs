use std::fs;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_NONE_MATCH};

use crate::error::{Error, Result};

pub struct FetchResult {
    pub path: PathBuf,
    pub etag: Option<String>,
    pub was_cached: bool,
}

/// Fetch a URL to a local file, honouring ETag for conditional GET.
/// Returns `was_cached = true` if the server returned 304 Not Modified.
pub fn fetch_to_file(
    client: &Client,
    url: &str,
    dest: &Path,
    existing_etag: Option<&str>,
) -> Result<FetchResult> {
    fs::create_dir_all(dest.parent().unwrap_or(dest))?;

    let mut req = client.get(url);
    if let Some(etag) = existing_etag {
        if dest.exists() {
            req = req.header(IF_NONE_MATCH, etag);
        }
    }

    let response = req.send().map_err(|e| {
        Error::Network(format!("GET {url}: {e}"))
    })?;

    if response.status().as_u16() == 304 {
        // Not modified — use cached file.
        return Ok(FetchResult {
            path: dest.to_path_buf(),
            etag: existing_etag.map(String::from),
            was_cached: true,
        });
    }

    if !response.status().is_success() {
        return Err(Error::Network(format!(
            "GET {url} returned {}",
            response.status()
        )));
    }

    let new_etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let body = response.bytes().map_err(|e| Error::Network(e.to_string()))?;
    fs::write(dest, &body)?;

    Ok(FetchResult {
        path: dest.to_path_buf(),
        etag: new_etag,
        was_cached: false,
    })
}

/// Fetch the OBO Foundry JSON-LD registry.
pub fn fetch_obo_foundry_registry(client: &Client, dest: &Path) -> Result<String> {
    let url = "https://obofoundry.org/registry/ontologies.jsonld";
    fetch_to_file(client, url, dest, None)?;
    Ok(fs::read_to_string(dest)?)
}
