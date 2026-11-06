//! On-demand, sha-pinned artifact cache (ADR-0144): dumps and adocs are
//! fetched from neo4j-graph-examples into a gitignored cache dir on first
//! use and verified by sha256. Nothing large is committed to git.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use relate_test_support::hex;

use crate::manifest::Dataset;

/// Cache root: `RELATE_GRAPH_EXAMPLES_CACHE` env override, else
/// `target/graph-examples/` (gitignored via `target/`).
pub fn cache_root() -> PathBuf {
    std::env::var_os("RELATE_GRAPH_EXAMPLES_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/graph-examples"))
}

fn dump_path(root: &Path, dataset: &Dataset) -> PathBuf {
    root.join(format!("{}-50.dump", dataset.name))
}

fn adoc_path(root: &Path, dataset: &Dataset) -> PathBuf {
    root.join(format!("{}.adoc", dataset.name))
}

/// Ensure the dataset's dump is in the cache: return the path after the
/// sha256 verifies (fresh download or existing file). `fetch` is false when
/// the caller wants offline operation — a cold cache is then an error the
/// caller turns into a skip.
pub fn ensure_dump(dataset: &Dataset, fetch: bool) -> Result<PathBuf, String> {
    let root = cache_root();
    let path = dump_path(&root, dataset);
    ensure_artifact(&path, dataset.dump_url, dataset.dump_sha256, fetch)
}

/// Ensure the dataset's adoc is in the cache; adocs are small and follow
/// the upstream repo, so a run with network always refetches (a cached
/// adoc is used only offline).
pub fn ensure_adoc(dataset: &Dataset, fetch: bool) -> Result<PathBuf, String> {
    let root = cache_root();
    let path = adoc_path(&root, dataset);
    if !fetch && path.exists() {
        return Ok(path);
    }
    if !fetch {
        return Err(format!(
            "adoc for {} not cached and network is off",
            dataset.name
        ));
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("creating cache dir: {e}"))?;
    let body = http_get_text(dataset.adoc_url)?;
    std::fs::write(&path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// Cached, sha-verified artifact download.
fn ensure_artifact(path: &Path, url: &str, sha256: &str, fetch: bool) -> Result<PathBuf, String> {
    // A cache hit is an existing file whose sha256 matches; a stale or
    // tampered entry falls through to a refetch over it.
    if path.exists() && sha256_of(path)? == sha256 {
        return Ok(path.to_path_buf());
    }
    if !fetch {
        return Err(format!(
            "{} not cached (network off); run once with network to populate the cache",
            path.display()
        ));
    }
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
        .map_err(|e| format!("creating cache dir: {e}"))?;
    let bytes = http_get_bytes(url)?;
    let got = hex(&Sha256::digest(&bytes));
    if got != sha256 {
        return Err(format!(
            "sha256 mismatch for {url}: manifest says {sha256}, got {got} — update the manifest"
        ));
    }
    std::fs::write(path, &bytes).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path.to_path_buf())
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok(hex(&Sha256::digest(&bytes)))
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .call()
        .map_err(|e| format!("fetching {url}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(512 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading {url}: {e}"))?;
    Ok(bytes)
}

fn http_get_text(url: &str) -> Result<String, String> {
    let bytes = http_get_bytes(url)?;
    String::from_utf8(bytes).map_err(|e| format!("utf8 from {url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_paths_are_per_dataset() {
        let root = Path::new("/cache");
        assert!(dump_path(root, &crate::manifest::DATASETS[0])
            .to_string_lossy()
            .ends_with("-50.dump"));
        assert!(adoc_path(root, &crate::manifest::DATASETS[0])
            .to_string_lossy()
            .ends_with(".adoc"));
    }
}
