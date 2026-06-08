use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use ureq::Agent;

use crate::error::{Error, Result};

pub struct FetchResult {
    pub path: PathBuf,
    pub etag: Option<String>,
    pub was_cached: bool,
}

/// Build a ureq agent (reuse across requests).
pub fn build_agent() -> Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .build()
}

/// Fetch a URL to a local file, honouring ETag for conditional GET.
/// Returns `was_cached = true` if the server returned 304 Not Modified.
pub fn fetch_to_file(
    agent: &Agent,
    url: &str,
    dest: &Path,
    existing_etag: Option<&str>,
) -> Result<FetchResult> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut req = agent.get(url);
    if let Some(etag) = existing_etag {
        if dest.exists() {
            req = req.set("If-None-Match", etag);
        }
    }

    let response = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(304, _)) => {
            // Not modified — use cached file.
            return Ok(FetchResult {
                path: dest.to_path_buf(),
                etag: existing_etag.map(String::from),
                was_cached: true,
            });
        }
        Err(e) => {
            return Err(Error::Network(format!("GET {url}: {e}")));
        }
    };

    let new_etag = response.header("etag").map(String::from);
    let mut body: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| Error::Network(format!("reading response body: {e}")))?;

    fs::write(dest, &body)?;

    Ok(FetchResult {
        path: dest.to_path_buf(),
        etag: new_etag,
        was_cached: false,
    })
}

/// Fetch the OBO Foundry JSON-LD registry.
pub fn fetch_obo_foundry_registry(agent: &Agent, dest: &Path) -> Result<String> {
    let url = "https://obofoundry.org/registry/ontologies.jsonld";
    fetch_to_file(agent, url, dest, None)?;
    Ok(fs::read_to_string(dest)?)
}
