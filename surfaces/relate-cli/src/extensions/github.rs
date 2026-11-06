//! The `--github` tier's pin resolution (ADR-0449): a release tag (or a
//! full commit SHA) resolved through GitHub's REST API to the commit it
//! names, the host platform's package asset on that release, and the
//! asset's SHA256. Nothing here downloads the package — the asset fetch
//! is the probe-then-fallback route's job (`release_channel::download_route`),
//! and the pin decides *before* that fetch whether the install may
//! proceed.
//!
//! The three API documents are consumed narrowly and fail closed:
//!
//! - `GET {api}/repos/{repo}/commits/{ref}` → `sha` — the tag-to-commit
//!   resolution (annotated and lightweight tags both resolve to the commit
//!   they point at).
//! - `GET {api}/repos/{repo}/releases/tags/{tag}` → `tag_name`, and each
//!   `assets[]` entry's `name` and `digest` — the release a tag names.
//! - `GET {api}/repos/{repo}/releases` → the same document per release,
//!   scanned newest-first to find the release a `@commit` pin names.
//!
//! The package asset follows the first-party channel's naming convention
//! (`<name>-<version>-<os>-<arch>.tar.gz`, ADR-0431 §1): exactly one
//! asset of the release must carry the package name and the host
//! platform's suffix. The SHA256 comes from GitHub's own recorded
//! `assets[].digest` (`sha256:<hex>`); an asset without one is a
//! refusal, never an unverified install.

use serde::Deserialize;

use crate::extensions::provenance::Pin;
use crate::extensions::source::{is_full_commit_sha, GithubPin};
use crate::release_channel::http::{download_asset, AssetRequest, DownloadError};
use crate::release_channel::{ChannelError, Platform};

/// The media type the release documents are requested with (ADR-0358 §3's
/// release JSON representation).
const RELEASE_JSON_ACCEPT: &str = "application/vnd.github+json";

/// How many releases a `@commit` pin scans (GitHub's maximum page). A
/// commit whose release is older than the newest page is a refusal naming
/// the scan — the pin tier's documented bound.
const RELEASES_PER_PAGE: usize = 100;

/// A resolved GitHub pin (ADR-0449): what the release lookup answered —
/// the tag, its commit, the platform asset and the asset's SHA256. The
/// trust store and the provenance marker record the same four fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRelease {
    /// The release tag the asset belongs to.
    pub(crate) tag: String,
    /// The commit the tag resolved to (40 lowercase hex).
    pub(crate) commit: String,
    /// The platform package asset's name.
    pub(crate) asset: String,
    /// The asset's SHA256 (64 lowercase hex).
    pub(crate) asset_sha256: String,
}

impl ResolvedRelease {
    /// The recorded pin for `repo`.
    pub(crate) fn pin(&self, repo: &str) -> Pin {
        Pin {
            repo: repo.to_string(),
            tag: self.tag.clone(),
            commit: self.commit.clone(),
            asset_sha256: self.asset_sha256.clone(),
        }
    }
}

/// One `assets[]` entry of a release document (ADR-0449): only the name
/// and the GitHub-recorded digest are consumed; `id` is ignored (the
/// fallback download route resolves ids itself, by name, over the same
/// document).
#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    #[serde(default)]
    digest: Option<String>,
}

/// A release document as the two endpoints return it (ADR-0449): the tag
/// name and the assets. Unknown keys — the rest of the document and of
/// each asset object — are ignored.
#[derive(Debug, Deserialize)]
struct ReleaseDocument {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

/// The commit lookup's one consumed field.
#[derive(Debug, Deserialize)]
struct CommitDocument {
    sha: String,
}

/// A GitHub-resolution failure: one line, prefixed so it cannot be
/// mistaken for the download route's own failures.
fn api_err(detail: impl std::fmt::Display) -> ChannelError {
    ChannelError::new(format!("GitHub install: {detail}"))
}

/// Resolve `pin` for `repo`'s package `name` (ADR-0449): the commit the
/// tag names (or the commit pin itself) and the host platform's asset
/// with its SHA256. `api_base` is the REST API base the env seam
/// resolves (`RELATE_GITHUB_API_URL`, the test hook); `token` is the
/// optional `GITHUB_TOKEN`, raising the anonymous rate limit.
pub(crate) fn resolve(
    repo: &str,
    name: &str,
    pin: &GithubPin,
    api_base: &str,
    token: Option<&str>,
) -> Result<ResolvedRelease, ChannelError> {
    let platform = crate::release_channel::host_platform().ok_or_else(|| {
        api_err(format!(
            "this host ({}/{}) has no extension package platform; the GitHub pin tier \
             covers darwin and linux on amd64 and arm64",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    match pin {
        GithubPin::Tag(tag) => {
            let commit = fetch_commit(api_base, repo, tag, token)?;
            let release = fetch_release(api_base, repo, tag, token)?;
            resolve_release(repo, name, &release, commit, platform)
        }
        GithubPin::Commit(sha) => {
            let releases = fetch_releases(api_base, repo, token)?;
            let mut matching = Vec::new();
            for release in &releases {
                if fetch_commit(api_base, repo, &release.tag_name, token)? == *sha {
                    matching.push(release);
                }
            }
            match matching.as_slice() {
                [] => Err(api_err(format!(
                    "no release of {repo} among the newest {RELEASES_PER_PAGE} targets commit \
                     {sha}; pin the release tag instead"
                ))),
                [release] => resolve_release(repo, name, release, sha.clone(), platform),
                several => Err(api_err(format!(
                    "commit {sha} is targeted by more than one release of {repo} ({}); \
                     pin the release tag instead",
                    several
                        .iter()
                        .map(|release| release.tag_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
            }
        }
    }
}

/// The commit a release tag names (ADR-0449): `commits/{ref}` accepts tag
/// names (annotated and lightweight) and answers the commit they point
/// at.
fn fetch_commit(
    api_base: &str,
    repo: &str,
    refname: &str,
    token: Option<&str>,
) -> Result<String, ChannelError> {
    let url = format!("{api_base}/repos/{repo}/commits/{refname}");
    let json = get_json(&url, "the commit lookup", token)?;
    parse_commit(&json)
}

/// The release a tag names (ADR-0449).
fn fetch_release(
    api_base: &str,
    repo: &str,
    tag: &str,
    token: Option<&str>,
) -> Result<ReleaseDocument, ChannelError> {
    let url = format!("{api_base}/repos/{repo}/releases/tags/{tag}");
    let json = get_json(&url, "the release-tag lookup", token)?;
    parse_release(&json)
}

/// The newest page of releases, for a `@commit` pin's tag search
/// (ADR-0449).
fn fetch_releases(
    api_base: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<ReleaseDocument>, ChannelError> {
    let url = format!("{api_base}/repos/{repo}/releases?per_page={RELEASES_PER_PAGE}");
    let json = get_json(&url, "the release listing", token)?;
    parse_releases(&json)
}

/// One API document, fetched through the channel's shared HTTP client
/// (the same cap, agent and redirect-auth pin), decoded as UTF-8.
fn get_json(url: &str, label: &str, token: Option<&str>) -> Result<String, ChannelError> {
    let bytes = download_asset(AssetRequest {
        url,
        name: label,
        token,
        accept: Some(RELEASE_JSON_ACCEPT),
    })
    .map_err(|e| api_err(DownloadError::into_channel_error(e)))?;
    String::from_utf8(bytes).map_err(|e| api_err(format!("{label} response is not UTF-8: {e}")))
}

/// Parse the commit lookup (ADR-0449): the `sha` field, required to be a
/// full 40-hex commit. Pure: the fetch is a separate boundary.
fn parse_commit(json: &str) -> Result<String, ChannelError> {
    let document: CommitDocument = serde_json::from_str(json)
        .map_err(|e| api_err(format!("the commit lookup is malformed: {e}")))?;
    let sha = document.sha.to_ascii_lowercase();
    if !is_full_commit_sha(&sha) {
        return Err(api_err(format!(
            "the commit lookup answered '{sha}', not a full 40-hex commit sha"
        )));
    }
    Ok(sha)
}

/// Parse one release document (ADR-0449). This is deliberately not shared
/// with `release_channel::api_assets`'s `TagLookup`: that reader consumes
/// `assets[].id` for the fallback download route, this one consumes
/// `tag_name` and `assets[].digest`, and each pins its own failure
/// wording ("release-tag lookup is malformed" there, "the release-tag
/// lookup is malformed" here) — a shared reader would move both texts
/// into per-caller parameters without removing a decision. Pure.
fn parse_release(json: &str) -> Result<ReleaseDocument, ChannelError> {
    serde_json::from_str(json)
        .map_err(|e| api_err(format!("the release-tag lookup is malformed: {e}")))
}

/// Parse the release listing (ADR-0449). A document that is not a JSON
/// array of release documents is a refusal. Pure.
fn parse_releases(json: &str) -> Result<Vec<ReleaseDocument>, ChannelError> {
    serde_json::from_str(json)
        .map_err(|e| api_err(format!("the release listing is malformed: {e}")))
}

/// The host platform's package asset on `release` (ADR-0449): exactly one
/// asset named `<name>-…-<os>-<arch>.tar.gz` — the first-party channel's
/// package naming convention (ADR-0431 §1). Zero or several candidates
/// are refusals naming what was found. Pure.
fn select_asset<'a>(
    repo: &str,
    name: &str,
    release: &'a ReleaseDocument,
    platform: Platform,
) -> Result<&'a ReleaseAsset, ChannelError> {
    let prefix = format!("{name}-");
    let suffix = format!("-{}-{}.tar.gz", platform.os, platform.arch);
    let candidates: Vec<&ReleaseAsset> = release
        .assets
        .iter()
        .filter(|asset| asset.name.starts_with(&prefix) && asset.name.ends_with(&suffix))
        .collect();
    match candidates.as_slice() {
        [] => Err(api_err(format!(
            "release {repo}@{} carries no package asset for '{name}' on {}/{} \
             (expected {name}-<version>-{}-{}.tar.gz)",
            release.tag_name, platform.os, platform.arch, platform.os, platform.arch
        ))),
        [asset] => Ok(asset),
        several => Err(api_err(format!(
            "release {repo}@{} carries several package assets for '{name}' on {}/{} ({}); \
             keep exactly one",
            release.tag_name,
            platform.os,
            platform.arch,
            several
                .iter()
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The asset's SHA256 from GitHub's recorded digest (ADR-0449): the
/// digest must be `sha256:<64 hex>`; an asset without a digest is a
/// refusal — the pin cannot be recorded for bytes whose hash nobody
/// published. Pure.
fn asset_sha256(repo: &str, tag: &str, asset: &ReleaseAsset) -> Result<String, ChannelError> {
    let Some(digest) = asset.digest.as_deref() else {
        return Err(api_err(format!(
            "release asset '{}' of {repo}@{tag} carries no digest; upload it through the \
             GitHub API or web UI so GitHub records its sha256",
            asset.name
        )));
    };
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(api_err(format!(
            "release asset '{}' of {repo}@{tag} carries the digest '{digest}', not \
             sha256:<hex>",
            asset.name
        )));
    };
    let hex = hex.to_ascii_lowercase();
    if !is_full_sha256(&hex) {
        return Err(api_err(format!(
            "release asset '{}' of {repo}@{tag} carries a malformed sha256 digest '{digest}'",
            asset.name
        )));
    }
    Ok(hex)
}

/// Whether `value` is 64 hex characters — a SHA256 in the pin's spelling.
fn is_full_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Turn a release into the resolved pin (ADR-0449): the host platform's
/// asset and its digest over the already-resolved commit.
fn resolve_release(
    repo: &str,
    name: &str,
    release: &ReleaseDocument,
    commit: String,
    platform: Platform,
) -> Result<ResolvedRelease, ChannelError> {
    let asset = select_asset(repo, name, release, platform)?;
    let asset_sha256 = asset_sha256(repo, &release.tag_name, asset)?;
    Ok(ResolvedRelease {
        tag: release.tag_name.clone(),
        commit,
        asset: asset.name.clone(),
        asset_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn release_json(tag: &str, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","name":"{tag}","assets":[{assets}],"prerelease":false}}"#)
    }

    fn asset_json(name: &str, digest: Option<&str>) -> String {
        match digest {
            Some(digest) => format!(r#"{{"name":"{name}","id":7,"digest":"{digest}"}}"#),
            None => format!(r#"{{"name":"{name}","id":7}}"#),
        }
    }

    fn host() -> Platform {
        crate::release_channel::host_platform().expect("the test hosts are darwin/linux")
    }

    #[test]
    fn a_commit_lookup_parses_the_full_sha() {
        assert_eq!(
            parse_commit(&format!(
                r#"{{"sha":"{COMMIT}","commit":{{"message":"x"}}}}"#
            ))
            .unwrap(),
            COMMIT
        );
        assert!(parse_commit(r#"{"sha":"abc"}"#)
            .unwrap_err()
            .to_string()
            .contains("not a full 40-hex commit sha"));
        assert!(parse_commit("{ nope")
            .unwrap_err()
            .to_string()
            .contains("malformed"));
    }

    #[test]
    fn the_host_platform_asset_is_selected_by_name_and_digest() {
        let platform = host();
        let json = release_json(
            "v1.0.0",
            &[
                asset_json(
                    "greet-1.0.0-x86_64-unknown.tar.gz",
                    Some(&format!("sha256:{SHA}")),
                ),
                asset_json(
                    &format!("greet-1.0.0-{}-{}.tar.gz", platform.os, platform.arch),
                    Some(&format!("sha256:{SHA}")),
                ),
                asset_json("notes.txt", None),
            ]
            .join(","),
        );
        let release = parse_release(&json).unwrap();
        let resolved = resolve_release(
            "owner/repo",
            "greet",
            &release,
            COMMIT.to_string(),
            platform,
        )
        .unwrap();
        assert_eq!(
            resolved,
            ResolvedRelease {
                tag: "v1.0.0".to_string(),
                commit: COMMIT.to_string(),
                asset: format!("greet-1.0.0-{}-{}.tar.gz", platform.os, platform.arch),
                asset_sha256: SHA.to_string(),
            }
        );
    }

    #[test]
    fn a_release_without_the_platform_asset_refuses() {
        let platform = host();
        let release = parse_release(&release_json(
            "v1.0.0",
            &asset_json(
                "greet-1.0.0-other-os.tar.gz",
                Some(&format!("sha256:{SHA}")),
            ),
        ))
        .unwrap();
        let err = select_asset("owner/repo", "greet", &release, platform)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("carries no package asset for 'greet'"),
            "{err}"
        );
        assert!(err.contains(platform.os), "{err}");
    }

    #[test]
    fn two_platform_candidates_refuse_naming_both() {
        let platform = host();
        let one = format!("greet-1.0.0-{}-{}.tar.gz", platform.os, platform.arch);
        let two = format!("greet-2.0.0-{}-{}.tar.gz", platform.os, platform.arch);
        let release = parse_release(&release_json(
            "v1.0.0",
            &[
                asset_json(&one, Some(&format!("sha256:{SHA}"))),
                asset_json(&two, Some(&format!("sha256:{SHA}"))),
            ]
            .join(","),
        ))
        .unwrap();
        let err = select_asset("owner/repo", "greet", &release, platform)
            .unwrap_err()
            .to_string();
        assert!(err.contains("several package assets"), "{err}");
        assert!(err.contains(&one) && err.contains(&two), "{err}");
    }

    #[test]
    fn a_missing_or_malformed_digest_refuses() {
        let platform = host();
        let name = format!("greet-1.0.0-{}-{}.tar.gz", platform.os, platform.arch);
        for (digest, needle) in [
            (None, "carries no digest"),
            (Some("md5:abcd"), "not sha256"),
            (Some("sha256:abc"), "malformed sha256"),
        ] {
            let release =
                parse_release(&release_json("v1.0.0", &asset_json(&name, digest))).unwrap();
            let err = resolve_release(
                "owner/repo",
                "greet",
                &release,
                COMMIT.to_string(),
                platform,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains(needle), "{digest:?}: {err}");
        }
    }

    #[test]
    fn the_release_listing_parses_an_array() {
        let json = format!(
            "[{},{}]",
            release_json("v2", &asset_json("x", Some(&format!("sha256:{SHA}")))),
            release_json("v1", "")
        );
        let releases = parse_releases(&json).unwrap();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag_name, "v2");
        assert!(parse_releases(r#"{"tag_name":"v1"}"#)
            .unwrap_err()
            .to_string()
            .contains("malformed"));
    }

    #[test]
    fn the_recorded_pin_carries_the_repo_and_the_resolution() {
        let resolved = ResolvedRelease {
            tag: "v1".to_string(),
            commit: COMMIT.to_string(),
            asset: "greet.tar.gz".to_string(),
            asset_sha256: SHA.to_string(),
        };
        assert_eq!(
            resolved.pin("owner/repo"),
            Pin {
                repo: "owner/repo".to_string(),
                tag: "v1".to_string(),
                commit: COMMIT.to_string(),
                asset_sha256: SHA.to_string(),
            }
        );
    }
}
