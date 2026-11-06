//! The first-party channel (ADR-0431 §1): the signed `extensions.json`
//! index at the website root beside `releases.json`, written only by the
//! release pipeline. One row per package: the name, its published
//! versions (newest first — a publisher contract; the default install
//! takes the first listed, `--version` must match one exactly), and for
//! each version its `protocol` and per-platform assets (the release tag,
//! the asset filename, the SHA256).
//!
//! Authentication is the upgrade chain generalized (ADR-0431 §1): the
//! index is signed by the one release signing key (ADR-0115: Ed25519
//! SSHSIG, namespace `relate-release`, key pinned in the binary,
//! never fetched), verified against the exact index bytes before
//! anything in the document is trusted — fail closed on a missing or
//! invalid signature, no signature-optional path. Platform scope
//! matches the release channel's (ADR-0068): macOS and Linux; a host
//! platform with no matching row refuses with an authored error naming
//! the platform and the package.
//!
//! `RELATE_EXTENSIONS_JSON_URL` overrides the index URL (the
//! documented dev/test seam, the ADR-0250 §1 pattern); the signature URL
//! derives from it (`<index>.sig`). Asset downloads ride the shared
//! probe-then-fallback route machinery (ADR-0358) through
//! [`crate::release_channel`].

use serde::Deserialize;

use relate_formatter::sanitize_error_text;

use crate::release_channel::http::{download_asset, AssetRequest, DownloadError};
use crate::release_channel::signing::verify;

use super::install::error::InstallError;

/// Canonical channel-index URL: the static `extensions.json` published to
/// the project website root, beside `releases.json` (ADR-0431 §1).
const EXTENSIONS_JSON_URL: &str = "https://relatedb.ai/extensions.json";

/// Environment override for the index URL (a documented dev/test seam,
/// ADR-0431's consequences — the ADR-0250 §1 pattern): the one way a test
/// can point the channel at a loopback fixture. Empty means the canonical
/// URL. The signature URL derives from the index URL.
const EXTENSIONS_JSON_URL_ENV: &str = "RELATE_EXTENSIONS_JSON_URL";

/// The channel-index document as serde derives it. Unknown keys are
/// ignored at every level — the releases.json forward-compat rule (ADR-0250
/// §2) carries over (ADR-0431 §1): the website schema can grow without
/// breaking older CLIs. Required keys are enforced by the derive, and the
/// values are checked where a bad one would otherwise flow by default.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChannelDocument {
    packages: Vec<ChannelPackage>,
}

/// One package row: the name, its published versions, and the hidden
/// policy (ADR-0429 §2 — a hidden package stays hidden in no listing, so
/// `--available` does not advertise it either).
#[derive(Debug, Clone, Deserialize)]
struct ChannelPackage {
    name: String,
    versions: Vec<ChannelVersion>,
    #[serde(default)]
    hidden: bool,
}

/// One published version: its own version string, the protocol it speaks
/// (the MCP revision, ADR-0447), and the per-platform assets.
#[derive(Debug, Clone, Deserialize)]
struct ChannelVersion {
    version: String,
    protocol: String,
    assets: Vec<ChannelAsset>,
}

/// One per-platform asset row: the installer's os/arch vocabulary, the
/// release tag that hosts the tarball, the asset filename, and its SHA256.
#[derive(Debug, Clone, Deserialize)]
struct ChannelAsset {
    os: String,
    arch: String,
    tag: String,
    asset: String,
    sha256: String,
}

/// The resolved install row (ADR-0431 §4 step 2): everything the channel
/// install needs — the host platform's asset, its release tag, and its
/// signed checksum. The name rides the caller's argument and the version
/// the manifest carries (the loud line reads it back), so neither
/// travels on the row.
#[derive(Debug)]
pub(crate) struct ChannelRow<'a> {
    /// The release tag hosting the tarball (its leading `v` included, as
    /// the tag-lookup route expects).
    pub(crate) tag: &'a str,
    /// The tarball's release-asset filename.
    pub(crate) asset: &'a str,
    /// The tarball's SHA256, from the signed index (lowercase hex; an
    /// uppercase row is tolerated and normalized, the SHA256SUMS
    /// precedent).
    pub(crate) sha256: String,
}

impl ChannelPackage {
    /// The package's listed versions joined for refusal texts (an unlisted
    /// `--version` names them): newest first, the publisher's order.
    fn listed_versions(&self) -> String {
        self.versions
            .iter()
            .map(|v| v.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The index URL to read: `RELATE_EXTENSIONS_JSON_URL` when set and
/// non-empty, else the canonical website URL — the shared seam policy
/// (ADR-0250 §1, ADR-0358 §8), resolved by
/// [`crate::release_channel::seam`]. The signature URL for an index at
/// `url`: `<url>.sig` (ADR-0431 §1 — the index's signature sits beside it
/// on the website). Pure.
fn signature_url(url: &str) -> String {
    format!("{url}.sig")
}

/// A malformed-index refusal (ADR-0431's consequences: every failure path
/// has a pinned, tested text).
fn malformed(detail: impl std::fmt::Display) -> InstallError {
    InstallError::Runtime(format!("extensions.json is malformed: {detail}"))
}

/// Fetch the channel index and its signature, verify the SSHSIG against
/// the pinned release key over the exact index bytes, and decode the
/// document (ADR-0431 §4 step 1). The two requests carry no
/// `Authorization` header — the website is a different, public origin
/// (the releases.json rule, ADR-0250 §5). Fail closed on a missing or
/// invalid signature; nothing in the document is trusted before it.
pub(crate) fn fetch_verified_index() -> Result<ChannelDocument, InstallError> {
    let url = crate::release_channel::seam(EXTENSIONS_JSON_URL_ENV, EXTENSIONS_JSON_URL)?;
    let index = download_asset(AssetRequest {
        url: &url,
        name: "extensions.json",
        token: None,
        accept: None,
    })
    .map_err(|e| channel_read_err("index", &url, e))?;
    let sig_url = signature_url(&url);
    let sig = download_asset(AssetRequest {
        url: &sig_url,
        name: "extensions.json.sig",
        token: None,
        accept: None,
    })
    .map_err(|e| channel_read_err("signature", &sig_url, e))?;
    verify_index_signature(&url, &index, &sig)?;
    decode_index(&index)
}

/// A channel-document read refusal (ADR-0431 §4 step 1): one line naming
/// which channel artifact failed to read, its URL and the network cause —
/// the parameterized twin the index and signature reads share (the
/// offline shape ADR-0431 §9 pins for `--available`; install appends the
/// `--from` remedy at its call site).
fn channel_read_err(what: &str, url: &str, e: DownloadError) -> InstallError {
    InstallError::Runtime(format!(
        "could not read the extension channel {what} from {url}: {}",
        e.into_channel_error()
    ))
}

/// Verify the index signature over the exact index bytes (ADR-0431 §4
/// step 1): the shared signing root runs the check; this wrapper renders
/// the installer's pinned wording, which names the artifact
/// (`extensions.json.sig`).
fn verify_index_signature(url: &str, index: &[u8], sig: &[u8]) -> Result<(), InstallError> {
    use crate::release_channel::signing::SigningError;
    match verify(index, sig) {
        Ok(()) => Ok(()),
        Err(SigningError::InvalidKey(e)) => Err(InstallError::Runtime(format!(
            "the release signing key is invalid: {e}"
        ))),
        Err(SigningError::MalformedSignature(e)) => Err(InstallError::Runtime(format!(
            "extensions.json.sig from {url} is not a valid SSHSIG: {e}"
        ))),
        Err(SigningError::VerificationFailed(e)) => Err(InstallError::Runtime(format!(
            "extensions.json.sig from {url} failed verification against the pinned release key: {e} \
             (the channel index is not trusted; not installing)"
        ))),
    }
}

/// Decode the index document (fail closed, naming the offending value):
/// every package must list at least one version, and every version must
/// list at least one platform asset carrying a 64-hex SHA256 — the values
/// the resolution steps below would otherwise flow by default.
///
/// `pub(crate)` beyond the module's own tests for the install flow's
/// fixture-index driver (`extensions/install/tests.rs`): the channel e2e
/// stops at the signature gate (the pinned release key never verifies a
/// fixture-key signature, and the ADR pins no key seam), so the
/// post-signature steps are unit-driven over the decoded fixture index.
pub(crate) fn decode_index(bytes: &[u8]) -> Result<ChannelDocument, InstallError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| malformed(format!("the document is not UTF-8: {e}")))?;
    let document: ChannelDocument =
        serde_json::from_str(text).map_err(|e| malformed(e.to_string()))?;
    for package in &document.packages {
        if package.versions.is_empty() {
            return Err(malformed(format!(
                "package '{}' lists no versions",
                package.name
            )));
        }
        for version in &package.versions {
            if version.assets.is_empty() {
                return Err(malformed(format!(
                    "package '{}' version '{}' lists no platform assets",
                    package.name, version.version
                )));
            }
            for asset in &version.assets {
                if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(malformed(format!(
                        "package '{}' version '{}' asset '{}' carries an invalid sha256 '{}'",
                        package.name, version.version, asset.asset, asset.sha256
                    )));
                }
            }
        }
    }
    Ok(document)
}

// The host platform in the installer's os/arch vocabulary is the release
// channel's one table (`crate::release_channel::platform_for`, consulted
// through `crate::release_channel::host_platform()`): the upgrade's
// platform-binary picking and the index's per-platform extension rows
// speak the same vocabulary, so the mapping is shared, not re-tabled per
// flow (ADR-0431 §1 — the os/arch picking the upgrade already does,
// generalized to the extension assets).

/// Resolve the install row (ADR-0431 §4 step 2): the name (an unknown
/// name is an authored error naming the package and the channel), the
/// version (`--version` or the newest listed — an unlisted `--version` is
/// an authored refusal naming the listed ones), and the host platform's
/// asset row — the tag, the asset, and the checksum the install downloads
/// and verifies against. The protocol check (§8) fires here, before the
/// tarball download: the row's protocol must be one this host speaks,
/// else an authored refusal naming both numbers — the
/// fail-closed-no-negotiation rule, caught one boundary earlier.
pub(crate) fn resolve_row<'a>(
    document: &'a ChannelDocument,
    name: &str,
    version: Option<&str>,
) -> Result<ChannelRow<'a>, InstallError> {
    let package = document
        .packages
        .iter()
        .find(|package| package.name == name)
        .ok_or_else(|| unknown_package(document, name))?;
    let version = match version {
        Some(requested) => package
            .versions
            .iter()
            .find(|v| v.version == requested)
            .ok_or_else(|| {
                InstallError::Runtime(format!(
                    "version '{requested}' of '{name}' is not listed in the channel index (listed: {})",
                    package.listed_versions()
                ))
            })?,
        // Newest listed (the publisher lists newest first).
        None => &package.versions[0],
    };
    // §8, pre-download: the index's protocol column lets the refusal fire
    // before the tarball download (ADR-0447: the column names the MCP
    // revision the package speaks).
    if version.protocol != relate_wire::mcp::MCP_PROTOCOL_VERSION {
        return Err(InstallError::Runtime(format!(
            "package '{name}' {} speaks protocol '{}', but this host speaks '{}'",
            version.version,
            version.protocol,
            relate_wire::mcp::MCP_PROTOCOL_VERSION
        )));
    }
    let Some(platform) = crate::release_channel::host_platform() else {
        return Err(InstallError::Runtime(format!(
            "the extension channel serves macOS and Linux only; this host is {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };
    let asset = version
        .assets
        .iter()
        .find(|asset| asset.os == platform.os && asset.arch == platform.arch)
        .ok_or_else(|| {
            InstallError::Runtime(format!(
                "the channel index lists no {}/{} asset for '{name}' {} \
                 (the install needs a first-party package for this platform)",
                platform.os, platform.arch, version.version
            ))
        })?;
    Ok(ChannelRow {
        tag: &asset.tag,
        asset: &asset.asset,
        sha256: asset.sha256.to_ascii_lowercase(),
    })
}

/// Copy one listed asset onto this host when the document has no row for
/// it. Signed fixtures and the publisher's golden index ship the two
/// release assets they stub (darwin/arm64 and linux/amd64); a linux/arm64
/// test host still has to resolve. The checksum and the tag stay the
/// donor's — the loopback serves that one tarball for every platform.
#[cfg(test)]
pub(crate) fn with_this_host(mut document: ChannelDocument) -> ChannelDocument {
    let Some(host) = crate::release_channel::host_platform() else {
        return document;
    };
    for package in &mut document.packages {
        for version in &mut package.versions {
            if version
                .assets
                .iter()
                .any(|asset| asset.os == host.os && asset.arch == host.arch)
            {
                continue;
            }
            let Some(donor) = version.assets.first().cloned() else {
                continue;
            };
            let needle = format!("-{}-{}", donor.os, donor.arch);
            let replacement = format!("-{}-{}", host.os, host.arch);
            let mut row = donor;
            row.asset = row.asset.replacen(&needle, &replacement, 1);
            row.os = host.os.to_string();
            row.arch = host.arch.to_string();
            version.assets.push(row);
        }
    }
    document
}

/// The unknown-package refusal (ADR-0431 §4 step 2): names the package and
/// the channel; the non-hidden names are listed when any exist — hidden
/// packages stay hidden in no listing (ADR-0429 §2's hidden policy).
fn unknown_package(document: &ChannelDocument, name: &str) -> InstallError {
    let visible: Vec<&str> = document
        .packages
        .iter()
        .filter(|package| !package.hidden)
        .map(|package| package.name.as_str())
        .collect();
    let listed = if visible.is_empty() {
        "the channel lists no packages".to_string()
    } else {
        format!("available: {}", visible.join(", "))
    };
    InstallError::Runtime(format!(
        "no package '{name}' in the extension channel index ({listed})"
    ))
}

/// `extension list --available` (ADR-0431 §9): one row per non-hidden
/// package — the name, its newest version, its `protocol`, and whether it
/// is installed — read from the verified index and the local registry's
/// installed-name set. Hidden packages stay hidden in no listing. The
/// index's own fragments (its names and versions arrive from a document
/// the website served) render sanitized, the listing rows' shared rule.
pub(crate) fn available_rows(
    document: &ChannelDocument,
    installed: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    document
        .packages
        .iter()
        .filter(|package| !package.hidden)
        .map(|package| {
            // Newest listed (the publisher's order); the decode already
            // refused an empty version list.
            let newest = &package.versions[0];
            format!(
                "{} {} protocol {} {}",
                sanitize_error_text(&package.name),
                sanitize_error_text(&newest.version),
                newest.protocol,
                if installed(&package.name) {
                    "installed"
                } else {
                    "not installed"
                }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-package index — one visible with two versions and both
    /// platform rows, one hidden — the mutation anchor for the decode and
    /// resolution tests.
    fn index_text() -> String {
        r#"{
  "schema_version": 1,
  "generated_at": "2026-10-28T00:00:00Z",
  "packages": [
    {
      "name": "git-to-graph",
      "hidden": false,
      "versions": [
        {
          "version": "1.1.0",
          "protocol": "2025-06-18",
          "assets": [
            {"os": "darwin", "arch": "arm64", "tag": "relate-extensions-git-to-graph-1.1.0", "asset": "git-to-graph-1.1.0-darwin-arm64.tar.gz", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            {"os": "darwin", "arch": "amd64", "tag": "relate-extensions-git-to-graph-1.1.0", "asset": "git-to-graph-1.1.0-darwin-amd64.tar.gz", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            {"os": "linux", "arch": "amd64", "tag": "relate-extensions-git-to-graph-1.1.0", "asset": "git-to-graph-1.1.0-linux-amd64.tar.gz", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
            {"os": "linux", "arch": "arm64", "tag": "relate-extensions-git-to-graph-1.1.0", "asset": "git-to-graph-1.1.0-linux-arm64.tar.gz", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
          ]
        },
        {
          "version": "1.0.0",
          "protocol": "2025-06-18",
          "assets": [
            {"os": "darwin", "arch": "arm64", "tag": "relate-extensions-git-to-graph-1.0.0", "asset": "git-to-graph-1.0.0-darwin-arm64.tar.gz", "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"},
            {"os": "darwin", "arch": "amd64", "tag": "relate-extensions-git-to-graph-1.0.0", "asset": "git-to-graph-1.0.0-darwin-amd64.tar.gz", "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"},
            {"os": "linux", "arch": "amd64", "tag": "relate-extensions-git-to-graph-1.0.0", "asset": "git-to-graph-1.0.0-linux-amd64.tar.gz", "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"},
            {"os": "linux", "arch": "arm64", "tag": "relate-extensions-git-to-graph-1.0.0", "asset": "git-to-graph-1.0.0-linux-arm64.tar.gz", "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}
          ]
        }
      ]
    },
    {"name": "egg", "hidden": true, "versions": [{"version": "9.9.9", "protocol": "2025-06-18", "assets": [{"os": "darwin", "arch": "arm64", "tag": "t", "asset": "egg.tar.gz", "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}]}]}
  ]
}"#
        .to_string()
    }

    fn decoded() -> ChannelDocument {
        decode_index(index_text().as_bytes()).unwrap()
    }

    #[test]
    fn the_index_decodes_and_unknown_keys_are_ignored() {
        // Forward compat (ADR-0250 §2's rule carried over): additive keys
        // at every level must not break the decode.
        let with_unknown = index_text()
            .replacen(
                "\"schema_version\": 1,",
                "\"schema_version\": 1, \"website_build\": \"x\",",
                1,
            )
            .replacen(
                "{\"os\": \"darwin\", \"arch\": \"arm64\", \"tag\": \"relate-extensions-git-to-graph-1.1.0\"",
                "{\"os\": \"darwin\", \"arch\": \"arm64\", \"size\": 4096, \"tag\": \"relate-extensions-git-to-graph-1.1.0\"",
                1,
            );
        assert_ne!(with_unknown, index_text());
        assert!(decode_index(with_unknown.as_bytes()).is_ok());
    }

    #[test]
    fn decode_refuses_each_malformed_shape() {
        let not_utf8: Vec<u8> = vec![0xff, 0xfe];
        for (case, text, needle) in [
            ("not UTF-8", not_utf8.as_slice(), "is not UTF-8"),
            (
                "invalid JSON",
                b"not json" as &[u8],
                "extensions.json is malformed",
            ),
            ("missing packages", b"{}" as &[u8], "missing field `packages`"),
            (
                "a package with no versions",
                br#"{"packages": [{"name": "ghost", "versions": []}]}"# as &[u8],
                "package 'ghost' lists no versions",
            ),
            (
                "a version with no assets",
                br#"{"packages": [{"name": "x", "versions": [{"version": "1", "protocol": "2025-06-18", "assets": []}]}]}"# as &[u8],
                "lists no platform assets",
            ),
            (
                "an invalid sha256",
                br#"{"packages": [{"name": "x", "versions": [{"version": "1", "protocol": "2025-06-18", "assets": [{"os": "darwin", "arch": "arm64", "tag": "t", "asset": "a.tar.gz", "sha256": "xyz"}]}]}]}"# as &[u8],
                "an invalid sha256 'xyz'",
            ),
        ] {
            let err = decode_index(text).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "case {case}: {err}"
            );
        }
        // The counted refusal, pinned exactly: the offender and the
        // document both named.
        assert_eq!(
            decode_index(br#"{"packages": [{"name": "ghost", "versions": []}]}"#)
                .unwrap_err()
                .to_string(),
            "extensions.json is malformed: package 'ghost' lists no versions"
        );
    }

    #[test]
    fn resolve_row_selects_the_newest_version_by_default_and_the_exact_one_on_request() {
        let document = decoded();
        // The version selection is platform-independent; the platform row
        // is the host's. Every channel platform (darwin/linux,
        // arm64/amd64) has a row in both fixture versions.
        let row = resolve_row(&document, "git-to-graph", None).unwrap();
        // The selected version is pinned by its tag — the version string
        // itself rides the manifest (the loud line reads it back), so the
        // row carries only what the download needs.
        assert_eq!(row.tag, "relate-extensions-git-to-graph-1.1.0");
        let row = resolve_row(&document, "git-to-graph", Some("1.0.0")).unwrap();
        assert_eq!(row.tag, "relate-extensions-git-to-graph-1.0.0");
        assert_eq!(row.sha256.len(), 64);
    }

    #[test]
    fn resolve_row_names_the_listed_versions_on_an_unlisted_request() {
        let document = decoded();
        let err = resolve_row(&document, "git-to-graph", Some("2.0.0")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "version '2.0.0' of 'git-to-graph' is not listed in the channel index (listed: 1.1.0, 1.0.0)"
        );
    }

    #[test]
    fn resolve_row_names_the_package_and_the_visible_channel_on_an_unknown_name() {
        let document = decoded();
        let err = resolve_row(&document, "no-such", None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no package 'no-such' in the extension channel index (available: git-to-graph)"
        );
        // Hidden packages stay hidden in no listing (ADR-0429 §2): 'egg'
        // is never advertised.
        let rendered = err.to_string();
        assert!(!rendered.contains("egg"), "{rendered}");
    }

    #[test]
    fn resolve_row_refuses_a_protocol_this_host_does_not_speak() {
        // §8 pre-download: the index's protocol column lets the refusal
        // fire before the tarball download, naming both numbers.
        let text = index_text().replacen(
            "\"version\": \"1.1.0\",\n          \"protocol\": \"2025-06-18\",",
            "\"version\": \"1.1.0\",\n          \"protocol\": \"1999-01-01\",",
            1,
        );
        assert_ne!(text, index_text());
        let document = decode_index(text.as_bytes()).unwrap();
        let err = resolve_row(&document, "git-to-graph", None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "package 'git-to-graph' 1.1.0 speaks protocol '1999-01-01', but this host speaks '2025-06-18'"
        );
    }

    #[test]
    fn resolve_row_refuses_a_version_with_no_row_for_this_platform() {
        // A host platform with no matching row refuses naming the platform
        // and the package (ADR-0431 §1) — spelled through a version whose
        // assets serve only the other platform of this host pair.
        let host = crate::release_channel::host_platform()
            .expect("the test hosts are darwin/linux arm64/amd64");
        let other = if (host.os, host.arch) == ("darwin", "arm64") {
            ("linux", "amd64")
        } else {
            ("darwin", "arm64")
        };
        let single = format!(
            r#"{{"packages": [{{"name": "one-platform", "versions": [{{"version": "1", "protocol": "2025-06-18", "assets": [{{"os": "{}", "arch": "{}", "tag": "t", "asset": "a.tar.gz", "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}}]}}]}}]}}"#,
            other.0, other.1
        );
        let document = decode_index(single.as_bytes()).unwrap();
        let err = resolve_row(&document, "one-platform", None).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "the channel index lists no {}/{} asset for 'one-platform' 1 \
                 (the install needs a first-party package for this platform)",
                host.os, host.arch
            )
        );
    }

    #[test]
    fn available_rows_list_non_hidden_packages_with_install_state() {
        let document = decoded();
        let rows = available_rows(&document, &|name| name == "git-to-graph");
        assert_eq!(
            rows,
            vec!["git-to-graph 1.1.0 protocol 2025-06-18 installed".to_string()]
        );
        // A channel with only hidden packages lists nothing.
        let hidden_only = r#"{"packages": [{"name": "egg", "hidden": true, "versions": [{"version": "1", "protocol": "2025-06-18", "assets": [{"os": "darwin", "arch": "arm64", "tag": "t", "asset": "a", "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}]}]}]}"#;
        let document = decode_index(hidden_only.as_bytes()).unwrap();
        assert_eq!(available_rows(&document, &|_| false), Vec::<String>::new());
    }

    #[test]
    fn the_signature_url_sits_beside_the_index_url() {
        // The seam policy itself (set and non-empty wins, else the
        // canonical URL) is pinned once in release_channel, over
        // `classify_env`; here only the channel's own derivation is.
        assert_eq!(
            signature_url("http://127.0.0.1:9/extensions.json"),
            "http://127.0.0.1:9/extensions.json.sig"
        );
        assert_eq!(
            signature_url(EXTENSIONS_JSON_URL),
            "https://relatedb.ai/extensions.json.sig"
        );
    }

    #[test]
    fn the_index_signature_failure_names_the_artifact_and_the_key() {
        // The typed root's failures render through the installer's wording:
        // the artifact is extensions.json.sig, the key is the pinned one. A
        // malformed blob is deterministic here; a signature that fails
        // against the pinned key (a fixture-key signature over these
        // bytes) is pinned end to end by the e2e bad-signature test.
        let err =
            verify_index_signature("http://x/extensions.json", b"{}", b"not a pem").unwrap_err();
        let rendered = err.to_string();
        assert!(
            rendered.starts_with(
                "extensions.json.sig from http://x/extensions.json is not a valid SSHSIG"
            ),
            "{rendered}"
        );
    }

    /// The release-workflow publisher's committed expected output
    /// (`.github/scripts/tests/fixtures/publish-extensions-json-expected.json`),
    /// produced by its bats suite against stubbed `gh` data — the same
    /// pin `e2e_upgrade` holds for `releases.json`: the decoder must
    /// accept the real producer's document, not only the CLI's own
    /// fixture, so the two sides of the ADR-0431 §1 channel-index contract
    /// stay pinned together.
    const GENERATOR_EXPECTED_EXTENSIONS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/scripts/tests/fixtures/publish-extensions-json-expected.json"
    ));

    #[test]
    fn the_publisher_expected_document_decodes_and_resolves() {
        // The producer's document — the generator's pinned expected output —
        // decodes through the real path, and its one package resolves the
        // host platform's row on every supported test host: the publisher
        // and the installer agree on the schema, not just on paper.
        // The golden file ships the two assets its stubbed releases
        // contain. A host outside that pair still resolves: the copied
        // row keeps the donor's checksum and retargets the filename.
        let document = with_this_host(
            decode_index(GENERATOR_EXPECTED_EXTENSIONS_JSON.as_bytes())
                .expect("the publisher's expected output decodes"),
        );
        let row = resolve_row(&document, "greet", None).expect("the fixture package resolves");
        assert_eq!(row.tag, "relate-extensions-greet-1.2.3");
        let host = crate::release_channel::host_platform()
            .expect("the test hosts are darwin/linux arm64/amd64");
        assert_eq!(
            row.asset,
            format!("greet-1.2.3-{}-{}.tar.gz", host.os, host.arch)
        );
        assert_eq!(row.sha256.len(), 64);
    }
}
