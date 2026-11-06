//! The authenticated GitHub API asset route (ADR-0358 §3): the fallback a
//! release-channel download takes when the direct browser download route
//! answers 404 (the repository is internal, so the direct route does not
//! honor tokens). It resolves the release tag's asset ids from
//! `GET {api_base}/repos/{repo}/releases/tags/{tag}` — consuming only
//! `assets[].name` and `assets[].id`, and failing closed when any asset
//! the caller needs is absent — then downloads each asset from
//! `GET {api_base}/repos/{repo}/releases/assets/{id}` with
//! `Accept: application/octet-stream` and the bearer token. The
//! repository slug is a parameter (ADR-0449: `--github` installs any
//! release's assets); the first-party flow passes [`super::REPO`], and
//! both endpoints of the route are built from it.
//!
//! The needed set is the caller's: `upgrade` needs its three assets
//! (ADR-0358 §1), `extension install` the one tarball the channel index
//! named (ADR-0431 §1) or the GitHub release's platform asset (ADR-0449)
//! — the route resolves whatever set it is given, by name, so no closed
//! vocabulary lives here.
//!
//! Everything here is pure except [`fetch_asset_ids`] and
//! [`download_asset_by_id`]; the route decision (the direct-route probe)
//! lives in [`super::download_route`], the shared HTTP download helper in
//! [`super::http`]. `RELATE_GITHUB_API_URL` overrides the API base, so
//! tests can point this route at a fixture server.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::http::{download_asset, AssetRequest, DownloadError};
use super::{seam, ChannelError};

/// Canonical GitHub API base (ADR-0358 §3): the origin of both the
/// release-tag lookup and the per-asset downloads.
const GITHUB_API_URL: &str = "https://api.github.com";

/// Environment override for the GitHub API base (a documented dev/test
/// seam, ADR-0358 §8): the way a test can point the fallback route at a
/// loopback fixture. Empty means the canonical base.
const GITHUB_API_URL_ENV: &str = "RELATE_GITHUB_API_URL";

/// The media type the release-tag lookup asks for (ADR-0358 §3): the REST
/// API's JSON representation of the release.
const RELEASE_JSON_ACCEPT: &str = "application/vnd.github+json";

/// The media type an asset download asks for (ADR-0358 §3): the raw asset
/// bytes, not the asset object's JSON metadata.
const ASSET_BYTES_ACCEPT: &str = "application/octet-stream";

/// The asset ids the API route downloads (ADR-0358 §3), resolved by exact
/// name from the release-tag lookup for the caller's needed set. Only
/// [`parse_asset_ids`] builds this, and it fails closed when a needed name
/// is absent, so a route built from these ids always has every name its
/// caller asks for.
#[derive(Debug)]
pub(crate) struct AssetIds {
    /// Asset name → API id, in the lookup's document order (BTreeMap only
    /// for a deterministic Debug rendering; lookups are by exact name).
    ids: BTreeMap<String, u64>,
}

impl AssetIds {
    /// The API id of the asset named `name`. The map was resolved for the
    /// caller's needed set and callers download only names from that set,
    /// so an absent name is an invariant violation, not a runtime case —
    /// the expect states it.
    pub(crate) fn id(&self, name: &str) -> u64 {
        *self
            .ids
            .get(name)
            .expect("the tag lookup resolved an id for every needed asset name")
    }
}

/// A release-tag lookup as serde derives it: only the `assets[]` fields
/// the route consumes (ADR-0358 §3). Unknown keys — the rest of the release
/// document and of each asset object — are ignored.
#[derive(Deserialize)]
struct TagLookup {
    assets: Vec<TagAsset>,
}

/// One `assets[]` entry of the release-tag lookup (ADR-0358 §3).
#[derive(Deserialize)]
struct TagAsset {
    name: String,
    id: u64,
}

/// A GitHub API route failure (ADR-0358 §3): one line, prefixed so it
/// cannot be mistaken for a direct-route failure.
fn api_err(detail: impl std::fmt::Display) -> ChannelError {
    ChannelError::new(format!("GitHub API asset route: {detail}"))
}

/// The GitHub API base to use: `RELATE_GITHUB_API_URL` when set and
/// non-empty (the documented dev/test seam), else the canonical
/// `https://api.github.com` (ADR-0358 §8). There is no fallback between
/// the two; a set but non-UTF-8 value is a typed error naming the
/// variable.
pub(crate) fn github_api_url() -> Result<String, ChannelError> {
    seam(GITHUB_API_URL_ENV, GITHUB_API_URL)
}

/// The release-tag lookup URL for `repo`'s `tag` under `base` (ADR-0358
/// §3, ADR-0449: any repository slug, not only the first-party one). The
/// tag keeps its leading `v`. Pure, so the URL shape is unit-testable.
fn tag_lookup_url(base: &str, repo: &str, tag: &str) -> String {
    format!("{base}/repos/{repo}/releases/tags/{tag}")
}

/// The per-asset download URL for `repo`'s asset `id` under `base`
/// (ADR-0358 §3). Pure, so the URL shape is unit-testable.
fn asset_url(base: &str, repo: &str, id: u64) -> String {
    format!("{base}/repos/{repo}/releases/assets/{id}")
}

/// Parse a release-tag lookup (ADR-0358 §3) into the asset ids for the
/// caller's `needed` names, consuming only `assets[].name` and
/// `assets[].id`. Fails closed when a needed name is absent from the
/// response. Pure: the HTTP fetch is a separate boundary.
///
/// The parse → require-object → derive-deserialize boundary is deliberately
/// written out rather than shared with `releases_json::parse_releases`'s
/// copy: the two documents pin different user-facing wording for both
/// failure modes (`invalid JSON` / `the document is not a JSON object`
/// there, `release-tag lookup is malformed` / `release-tag lookup is not
/// a JSON object` here), so a shared helper would only move both texts
/// into per-caller parameters without removing a decision.
fn parse_asset_ids(
    json: &str,
    repo: &str,
    tag: &str,
    needed: &[&str],
) -> Result<AssetIds, ChannelError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| api_err(format!("release-tag lookup is malformed: {e}")))?;
    if !value.is_object() {
        return Err(api_err("release-tag lookup is not a JSON object"));
    }
    let lookup = TagLookup::deserialize(&value)
        .map_err(|e| api_err(format!("release-tag lookup is malformed: {e}")))?;
    let mut ids = BTreeMap::new();
    for name in needed {
        let id = lookup
            .assets
            .iter()
            .find(|asset| asset.name == *name)
            .map(|asset| asset.id)
            .ok_or_else(|| {
                api_err(format!(
                    "release {tag} from {repo} does not list the asset {name}"
                ))
            })?;
        ids.insert((*name).to_string(), id);
    }
    Ok(AssetIds { ids })
}

/// Resolve the asset ids for `tag`'s `needed` names from the release-tag
/// lookup (ADR-0358 §3). `token` is required: this route is only entered
/// with `GITHUB_TOKEN` set, and it is sent as `Authorization: Bearer`.
pub(crate) fn fetch_asset_ids(
    base: &str,
    repo: &str,
    tag: &str,
    needed: &[&str],
    token: &str,
) -> Result<AssetIds, ChannelError> {
    let url = tag_lookup_url(base, repo, tag);
    let bytes = download_asset(AssetRequest {
        url: &url,
        name: "release-tag lookup",
        token: Some(token),
        accept: Some(RELEASE_JSON_ACCEPT),
    })
    .map_err(DownloadError::into_channel_error)?;
    let json = String::from_utf8(bytes)
        .map_err(|e| api_err(format!("GET {url}: response is not UTF-8: {e}")))?;
    parse_asset_ids(&json, repo, tag, needed)
}

/// Download asset `id` as raw bytes over the API route (ADR-0358 §3), asking
/// for the bytes with `Accept: application/octet-stream` and the bearer
/// token. The token rides only this request: the shared download client
/// pins `RedirectAuthHeaders::Never` in [`super::http`], so the token is
/// never forwarded across the redirect to the asset CDN (ADR-0358 §6).
pub(crate) fn download_asset_by_id(
    base: &str,
    repo: &str,
    id: u64,
    name: &str,
    token: &str,
) -> Result<Vec<u8>, ChannelError> {
    let url = asset_url(base, repo, id);
    download_asset(AssetRequest {
        url: &url,
        name,
        token: Some(token),
        accept: Some(ASSET_BYTES_ACCEPT),
    })
    .map_err(DownloadError::into_channel_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release_channel::REPO;

    /// A release-tag lookup in the REST API's shape, with the extra keys a
    /// real response carries: only `assets[].name` and `assets[].id` may be
    /// consumed (ADR-0358 §3).
    fn lookup_json() -> String {
        r#"{
  "tag_name": "v0.12.5",
  "name": "v0.12.5",
  "prerelease": false,
  "body": "notes",
  "assets": [
    {
      "url": "https://api.github.com/repos/relatedb/relate/releases/assets/654066831",
      "id": 654066831,
      "name": "relate-darwin-arm64",
      "size": 4096,
      "content_type": "application/octet-stream"
    },
    {
      "url": "https://api.github.com/repos/relatedb/relate/releases/assets/654066833",
      "id": 654066833,
      "name": "SHA256SUMS",
      "size": 453
    },
    {
      "url": "https://api.github.com/repos/relatedb/relate/releases/assets/654066835",
      "id": 654066835,
      "name": "SHA256SUMS.sig",
      "size": 232
    }
  ]
}"#
        .to_string()
    }

    /// The needed set of the parse tests: the upgrade's three assets.
    const NEEDED: [&str; 3] = ["relate-darwin-arm64", "SHA256SUMS", "SHA256SUMS.sig"];

    #[test]
    fn tag_lookup_and_asset_urls_build_from_the_base() {
        assert_eq!(
            tag_lookup_url("https://api.github.com", REPO, "v0.12.5"),
            "https://api.github.com/repos/relatedb/relate/releases/tags/v0.12.5"
        );
        assert_eq!(
            tag_lookup_url("http://127.0.0.1:9", REPO, "v0.1.0"),
            "http://127.0.0.1:9/repos/relatedb/relate/releases/tags/v0.1.0"
        );
        assert_eq!(
            asset_url("https://api.github.com", REPO, 654066833),
            "https://api.github.com/repos/relatedb/relate/releases/assets/654066833"
        );
        assert_eq!(
            asset_url("http://127.0.0.1:9", REPO, 7),
            "http://127.0.0.1:9/repos/relatedb/relate/releases/assets/7"
        );
    }

    #[test]
    fn parse_asset_ids_reads_name_and_id_pairs_for_the_needed_set() {
        let ids = parse_asset_ids(&lookup_json(), REPO, "v0.12.5", &NEEDED).unwrap();
        assert_eq!(ids.id("relate-darwin-arm64"), 654066831);
        assert_eq!(ids.id("SHA256SUMS"), 654066833);
        assert_eq!(ids.id("SHA256SUMS.sig"), 654066835);
        // A one-name needed set (the extension tarball's route) resolves
        // just that name.
        let tarball = parse_asset_ids(&lookup_json(), REPO, "v0.12.5", &["SHA256SUMS"]).unwrap();
        assert_eq!(tarball.id("SHA256SUMS"), 654066833);
    }

    #[test]
    fn parse_asset_ids_fails_closed_on_each_missing_needed_name() {
        // ADR-0358 §3: every needed asset must be listed; each one missing
        // is its own abort naming it.
        for missing in NEEDED.iter().copied() {
            let assets = NEEDED
                .iter()
                .filter(|name| **name != missing)
                .map(|name| format!(r#"{{"id": 7, "name": "{name}"}}"#))
                .collect::<Vec<_>>()
                .join(", ");
            let msg = parse_asset_ids(
                &format!(r#"{{"assets": [{assets}]}}"#),
                REPO,
                "v0.12.5",
                &NEEDED,
            )
            .unwrap_err()
            .to_string();
            assert_eq!(msg.lines().count(), 1, "one line: {msg:?}");
            assert!(
                msg.starts_with("GitHub API asset route: "),
                "unexpected error: {msg:?}"
            );
            assert!(
                msg.contains(&format!("does not list the asset {missing}")),
                "the abort must name {missing}: {msg:?}"
            );
            assert!(
                msg.contains("v0.12.5"),
                "the abort must name the tag: {msg:?}"
            );
        }
    }

    #[test]
    fn parse_asset_ids_rejects_malformed_documents() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "invalid JSON",
                "not json",
                "release-tag lookup is malformed",
            ),
            (
                "not a JSON object",
                "[]",
                "release-tag lookup is not a JSON object",
            ),
            ("missing assets", r#"{"tag_name": "v0.12.5"}"#, "assets"),
            (
                "assets is not an array",
                r#"{"assets": {}}"#,
                "release-tag lookup is malformed",
            ),
            (
                "asset id is a string",
                r#"{"assets": [{"name": "SHA256SUMS", "id": "7"}]}"#,
                "release-tag lookup is malformed",
            ),
            (
                "asset name is not a string",
                r#"{"assets": [{"name": 7, "id": 7}]}"#,
                "release-tag lookup is malformed",
            ),
        ];
        for (case, json, needle) in cases {
            let msg = parse_asset_ids(json, REPO, "v0.12.5", &NEEDED)
                .unwrap_err()
                .to_string();
            assert_eq!(msg.lines().count(), 1, "case {case}: one line: {msg:?}");
            assert!(
                msg.starts_with("GitHub API asset route: "),
                "case {case}: unexpected error: {msg:?}"
            );
            assert!(
                msg.contains(needle),
                "case {case}: expected {needle:?} in {msg:?}"
            );
        }
    }

    #[test]
    fn parse_asset_ids_ignores_unknown_keys_at_both_levels() {
        // ADR-0358 §3: only `assets[].name` and `assets[].id` are consumed;
        // additive REST API fields must not break the parse.
        let with_unknown = lookup_json()
            .replacen(
                "\"tag_name\"",
                "\"target_commitish\": \"main\", \"tag_name\"",
                1,
            )
            .replacen("\"size\": 453", "\"download_count\": 12, \"size\": 453", 1);
        assert_ne!(with_unknown, lookup_json());
        let ids = parse_asset_ids(&with_unknown, REPO, "v0.12.5", &NEEDED).unwrap();
        assert_eq!(ids.id("SHA256SUMS"), 654066833);
    }
}
