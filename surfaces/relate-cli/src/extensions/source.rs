//! The install source vocabulary (ADR-0431 §3, §5–§6, ADR-0449) — the one
//! typed sum, ungated so the `extension` grammar parses straight into it
//! in every build (ADR-0137): the parse produces the variant, the run
//! destructures it, and no nullable triple re-models the closed sum.
//! Compiled with `extensions` (the grammar's feature); the install flow
//! (`extension-install`) and the grammar share the one type — and the one
//! [`InstallArgs`] that carries the parsed name, source and force flag, so
//! the grammar hands the flow its own struct instead of a field copy.

use crate::extensions::provenance::Tier;

/// Where a package comes from (ADR-0431 §3, §5–§6; ADR-0449): the signed
/// channel, the GitHub release pinned to a commit, or one of the two
/// explicit trust acts — mutually exclusive at the grammar boundary, so
/// exactly one shape ever reaches the flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallSource {
    /// The first-party channel (§4): the signed index's row for `name`,
    /// `--version V` selecting it (default: the newest listed).
    Channel {
        /// `--version V`: the channel version to select, newest when
        /// absent.
        version: Option<String>,
    },
    /// `--github REPO@TAG|@SHA` (ADR-0449): a GitHub release pinned to a
    /// commit — the tag's commit and the platform asset's SHA256 are
    /// resolved and recorded, and the per-user trust store refuses a
    /// moved pin without an explicit re-approval.
    Github(GithubSpec),
    /// `--url URL` (§6): the explicit third-party trust act — the
    /// tarball at an https URL (or plain-http loopback) the invoker chose.
    Url(String),
    /// `--from DIR` (§5): the local directory trust act, `pull --from`'s
    /// posture.
    From(String),
}

impl InstallSource {
    /// The trust tier the source arrives by (ADR-0431 §7, ADR-0449): the
    /// tier is the source — the channel's signature, the GitHub pin's
    /// recorded commit, the URL's invocation, the directory's
    /// provisioning.
    pub(crate) fn tier(&self) -> Tier {
        match self {
            Self::Channel { .. } => Tier::FirstParty,
            Self::Github(_) => Tier::GithubPin,
            Self::Url(_) => Tier::ThirdParty,
            Self::From(_) => Tier::Local,
        }
    }
}

/// The `--github` value as parsed at the grammar boundary (ADR-0449): the
/// repository slug and the pin the invoker named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubSpec {
    /// `owner/repo`, exactly two non-empty segments.
    pub(crate) repo: String,
    /// The pin: a release tag or a full commit SHA.
    pub(crate) pin: GithubPin,
}

/// What the `--github` value pins (ADR-0449): the release tag the asset
/// belongs to, or the commit SHA itself. A bare `owner/repo` is a usage
/// error — an unpinned repository is a moving target — so this sum has no
/// "whatever is newest" variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GithubPin {
    /// `@TAG`: resolved to the tag's commit at install time.
    Tag(String),
    /// `@<full-commit-sha>`: the pin is the commit itself.
    Commit(String),
}

impl GithubSpec {
    /// Parse one `--github` value: `owner/repo@tag` or
    /// `owner/repo@<full-commit-sha>` (ADR-0449). The value is refused
    /// with the usage message when the repository is not exactly
    /// `owner/repo`, when the pin is missing (the untagged moving
    /// target), or when either segment is empty or carries a character
    /// no GitHub repository/ref segment can (whitespace, `/`, `\`, `@`).
    /// A 40-hex value is the commit pin; any other pin is a tag.
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let Some((repo, refname)) = value.split_once('@') else {
            return Err(format!(
                "--github '{value}' needs a pin: owner/repo@tag or \
                 owner/repo@<full-commit-sha> (an untagged repository is a moving target)"
            ));
        };
        let segments: Vec<&str> = repo.split('/').collect();
        let slug_ok = segments.len() == 2
            && segments
                .iter()
                .all(|segment| !segment.is_empty() && segment.chars().all(is_slug_char));
        if !slug_ok {
            return Err(format!(
                "--github '{value}': '{repo}' is not an owner/repo repository slug"
            ));
        }
        if refname.is_empty() {
            return Err(format!("--github '{value}': the pin after '@' is empty"));
        }
        if !refname.chars().all(is_slug_char) {
            return Err(format!(
                "--github '{value}': '{refname}' is not a tag name or full commit sha"
            ));
        }
        let pin = if is_full_commit_sha(refname) {
            GithubPin::Commit(refname.to_ascii_lowercase())
        } else {
            GithubPin::Tag(refname.to_string())
        };
        Ok(Self {
            repo: repo.to_string(),
            pin,
        })
    }
}

/// The characters a repository slug segment and a tag name share: ASCII
/// alphanumerics plus `.`, `_`, `-`, `+`. `@` and `/` are excluded (they
/// delimit the flag's own grammar); whitespace and control characters
/// would corrupt the URLs and the output line.
fn is_slug_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+')
}

/// Whether `value` is the full 40-hex commit SHA the `@sha` pin requires.
/// A 40-hex refname is always read as a commit — GitHub repository rules
/// allow such a tag, but the pin's stronger reading wins (ADR-0449).
pub(crate) fn is_full_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_tag_pin_parses_the_slug_and_the_tag() {
        assert_eq!(
            GithubSpec::parse("relatedb/relate-extensions@v1.2.3").unwrap(),
            GithubSpec {
                repo: "relatedb/relate-extensions".to_string(),
                pin: GithubPin::Tag("v1.2.3".to_string()),
            }
        );
    }

    #[test]
    fn a_full_commit_sha_is_the_commit_pin() {
        assert_eq!(
            GithubSpec::parse(&format!("owner/repo@{SHA}")).unwrap().pin,
            GithubPin::Commit(SHA.to_string())
        );
        // Uppercase hex normalizes: the comparison is byte equality.
        let upper = SHA.to_ascii_uppercase();
        assert_eq!(
            GithubSpec::parse(&format!("owner/repo@{upper}"))
                .unwrap()
                .pin,
            GithubPin::Commit(SHA.to_string())
        );
    }

    #[test]
    fn an_untagged_repository_is_a_usage_style_refusal() {
        let err = GithubSpec::parse("owner/repo").unwrap_err();
        assert!(err.contains("needs a pin"), "{err}");
        assert!(err.contains("moving target"), "{err}");
    }

    #[test]
    fn a_malformed_slug_refuses() {
        for value in [
            "@v1",
            "/repo@v1",
            "owner/@v1",
            "owner/repo/extra@v1",
            "own er/repo@v1",
            "owner/re po@v1",
        ] {
            let err = GithubSpec::parse(value).unwrap_err();
            assert!(
                err.contains("is not an owner/repo repository slug"),
                "{value}: {err}"
            );
        }
    }

    #[test]
    fn a_malformed_pin_refuses() {
        for value in ["owner/repo@", "owner/repo@v1.0/rc", "owner/repo@v 1"] {
            let err = GithubSpec::parse(value).unwrap_err();
            assert!(
                err.contains("is empty") || err.contains("is not a tag name"),
                "{value}: {err}"
            );
        }
        // A short hex-ish value is a tag, not a truncated commit pin.
        assert_eq!(
            GithubSpec::parse("owner/repo@abc123").unwrap().pin,
            GithubPin::Tag("abc123".to_string())
        );
    }
}

/// The parsed `install` arguments: the grammar `extension_cmd` owns — the
/// source is the typed [`InstallSource`] (parsed at the grammar boundary,
/// in every build), so the flow never re-matches a nullable pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallArgs {
    /// The package name: the positional, the channel row key, and — when
    /// the manifest agrees — the installed package directory's name.
    pub(crate) name: String,
    /// Where the package comes from (ADR-0431 §3, ADR-0449): the signed
    /// channel by default, a commit-pinned GitHub release, or one of the
    /// two explicit trust acts.
    pub(crate) source: InstallSource,
    /// `--force`: clobber a foreign target directory — one without the
    /// provenance marker (§3). Never a collision or cross-tier bypass.
    pub(crate) force: bool,
    /// `--re-approve` (ADR-0449): re-approve whatever the `--github` pin
    /// resolves to now, replacing a recorded pin that no longer matches.
    pub(crate) re_approve: bool,
    /// `--approve <commit-sha>` (ADR-0449): re-approve only when the pin
    /// resolves to exactly this commit.
    pub(crate) approve: Option<String>,
}
