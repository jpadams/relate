//! Shared generated-token basic auth for Relate network surfaces
//! (ADR-0232, mechanism ported from the Bolt surface's `auth.rs`,
//! ADR-0126). Engine-neutral except for the one HTTP failure recipe:
//! Bolt HELLO uses [`AuthConfig::check`], and every HTTP surface uses
//! [`AuthConfig::check_authorization`] plus [`AuthConfig::http_failure`]
//! ([`HttpAuthFailure`], [`UNAVAILABLE_MESSAGE`]). Stdio consumers skip
//! the challenge entirely: the parent process spawned the server, so
//! there is no trust boundary to cross.
//!
//! Public API:
//!
//! - [`AuthConfig`] — the resolved auth mode (`None`, one `Basic`
//!   credential, or [`AuthConfig::Custom`]), with [`AuthConfig::basic`],
//!   [`AuthConfig::check`], [`AuthConfig::check_authorization`],
//!   [`AuthConfig::http_failure`], [`AuthConfig::transaction_owner`],
//!   [`AuthConfig::banner_label`], and [`AuthConfig::verified_principal`].
//!   [`AuthConfig::check_basic`] is the bool view of
//!   [`AuthConfig::check_authorization`].
//! - [`resolve`] — resolution order `--auth user:password` flag, then
//!   `RELATE_AUTH=user:password`, else a generated token printed at
//!   startup (via the returned banner lines) and persisted as a
//!   `<dir>.auth-token` sibling (mode 0600, fresh per start for
//!   `:memory:`).
//! - [`env_credential`] — reads the `RELATE_AUTH` process variable at
//!   the binary's edge.
//! - [`token_path`] — the generated token file's path for a store URI
//!   (the `<path>.auth-token` sibling; `None` for `:memory:`), the one
//!   owner of the path fact.
//! - [`parse_basic_header`] — RFC 7617 `Authorization: Basic` header
//!   parser for the HTTP surfaces.
//! - [`REALM`] — the one challenge realm every HTTP surface shares.
//!
//! `none` (either surface) disables authentication. A present-but-malformed
//! value (no colon, empty side) is a loud startup error, never a silent
//! fallback. Credential comparison is constant-time.
//!
//! Dependency note: the RFC 7617 decoder uses the `base64` crate already
//! in the workspace (pinned 0.22, used by `relate-load`) instead of a
//! hand-rolled decoder — standard, tiny, and zero new transitive weight.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The RFC 7617 basic-auth realm every HTTP surface challenges with
/// (`WWW-Authenticate: Basic realm="relate"`). One shared constant so
/// the HTTP Query API and the MCP streamable-HTTP transport can never drift
/// apart on the realm a client is asked to authenticate against.
pub const REALM: &str = "relate";

/// The body message every HTTP surface serves on a failed credential check
/// (ADR-0139's rate-limited 401). One constant so the surfaces cannot drift.
pub const UNAUTHORIZED_MESSAGE: &str = "authentication required";

/// The body message every HTTP surface serves when the authenticator cannot
/// complete a check. One constant so the 502 recipe cannot drift.
pub const UNAVAILABLE_MESSAGE: &str = "authentication service unavailable";

/// The 401 recipe's challenge header, `(name, value)`: the one constructor
/// for `WWW-Authenticate: Basic realm="…"`, consumed by every HTTP surface
/// (the shared pre-body gate and the Query API's `unauthorized` alike), so
/// a realm or header-name change is one edit.
pub fn basic_challenge() -> (String, String) {
    (
        "WWW-Authenticate".to_string(),
        format!("Basic realm=\"{REALM}\""),
    )
}

/// Why a credential was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// The credential is absent or not accepted. HTTP answers 401.
    Rejected,
    /// The check could not be completed. HTTP answers 502.
    Unavailable,
}

/// The HTTP representation of an authentication failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpAuthFailure {
    /// HTTP status: 401 when credentials were rejected, 502 when their
    /// verifier could not complete.
    pub status: u16,
    /// Caller-safe response body message.
    pub message: &'static str,
    /// The challenge for a rejected credential. An unavailable verifier does
    /// not challenge the caller to retry the same credential.
    pub challenge: Option<(String, String)>,
}

/// A host-supplied credential check (ADR-0428).
///
/// The credential is a bearer token or a Bolt HELLO password. HTTP also
/// asks for the transaction owner: an opaque caller id the transaction
/// registry binds. `Ok(None)` records no owner.
/// [`AuthError::Rejected`] rejects the request.
/// [`AuthError::Unavailable`] means the owner could not be resolved.
/// The owner is not an engine principal.
pub trait Authenticator: Send + Sync {
    /// Whether `credential` is accepted. [`AuthError::Unavailable`] is an
    /// incomplete check, not a rejection.
    fn verify_credential(&self, credential: &str) -> Result<(), AuthError>;

    /// Header whose value is the transaction owner. `None` means this
    /// authenticator does not scope transactions.
    fn owner_header(&self) -> Option<&'static str> {
        None
    }

    /// The HTTP transaction owner.
    ///
    /// `credential` is the bearer token or Basic password
    /// [`Self::verify_credential`] just accepted. `header_value` is the
    /// [`Self::owner_header`] value when that header was present.
    ///
    /// Derive the owner from `credential`. Return `header_value` only when
    /// a trusted proxy stripped any client-supplied copy of that header
    /// and set it from an identity that proxy authenticated. A client that
    /// can choose the header can otherwise open, run, and roll back another
    /// caller's transaction.
    fn transaction_owner(
        &self,
        credential: &str,
        header_value: Option<&str>,
    ) -> Result<Option<String>, AuthError>;
}

/// How a network surface validates credentials.
#[derive(Clone)]
pub enum AuthConfig {
    /// No validation: any or no credentials pass (opt-out via `none`).
    None,
    /// A single preset basic-auth credential, compared on every check.
    Basic { user: String, password: String },
    /// A host-supplied check (ADR-0428).
    Custom(Arc<dyn Authenticator>),
}

impl PartialEq for AuthConfig {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (
                Self::Basic {
                    user: a,
                    password: b,
                },
                Self::Basic {
                    user: c,
                    password: d,
                },
            ) => a == c && b == d,
            (Self::Custom(a), Self::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for AuthConfig {}

impl AuthConfig {
    /// Whether any credential is required (the one predicate behind every
    /// "auth on?" decision — WebSocket origin defaults, banners).
    pub fn is_enabled(&self) -> bool {
        !matches!(self, AuthConfig::None)
    }
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthConfig::None => f.write_str("None"),
            AuthConfig::Basic { user, .. } => f
                .debug_struct("Basic")
                .field("user", user)
                .field("password", &"<redacted>")
                .finish(),
            AuthConfig::Custom(_) => f.write_str("Custom"),
        }
    }
}

impl AuthConfig {
    /// One preset basic-auth credential (ADR-0126).
    pub fn basic(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Basic {
            user: user.into(),
            password: password.into(),
        }
    }

    /// Validate a presented `(principal, credentials)` pair; `None` when
    /// the presentation carries no such field (a missing credential never
    /// passes under `Basic`). Wire-shape neutral: a Bolt surface passes
    /// the HELLO map's `principal`/`credentials` (after checking the
    /// scheme itself), an HTTP surface passes the pair from
    /// [`parse_basic_header`].
    pub fn check(&self, principal: Option<&str>, credentials: Option<&str>) -> bool {
        match self {
            AuthConfig::None => true,
            AuthConfig::Basic { user, password } => {
                // Bitwise `&`, never `&&`: both comparisons run on every
                // check — a short-circuit would skip the credential check
                // on a wrong principal, and the skipped work itself becomes
                // the timing signal.
                ct_eq(principal, user) & ct_eq(credentials, password)
            }
            // A custom check has no preset pair. Callers use
            // [`Self::check_authorization`] or the Bolt HELLO adapter.
            AuthConfig::Custom(_) => false,
        }
    }

    /// Validate an `Authorization` header: RFC 7617 Basic under `Basic`,
    /// and a bearer token or Basic password under [`AuthConfig::Custom`].
    pub fn check_authorization(&self, header: Option<&str>) -> Result<(), AuthError> {
        match self {
            AuthConfig::None => Ok(()),
            AuthConfig::Basic { .. } => {
                let Some((presented_user, presented_password)) =
                    header.and_then(parse_basic_header)
                else {
                    return Err(AuthError::Rejected);
                };
                if self.check(Some(&presented_user), Some(&presented_password)) {
                    Ok(())
                } else {
                    Err(AuthError::Rejected)
                }
            }
            AuthConfig::Custom(auth) => {
                let Some(header) = header else {
                    return Err(AuthError::Rejected);
                };
                let Some(credential) = presented_credential(header) else {
                    return Err(AuthError::Rejected);
                };
                auth.verify_credential(&credential)
            }
        }
    }

    /// The host-declared header whose value scopes a transaction. `None`
    /// when this mode records no owner.
    pub fn owner_header(&self) -> Option<&'static str> {
        match self {
            AuthConfig::Custom(auth) => auth.owner_header(),
            AuthConfig::None | AuthConfig::Basic { .. } => None,
        }
    }

    /// The HTTP transaction owner (ADR-0428). `None` and `Basic` have one
    /// principal, or none, so they record no owner and do not isolate
    /// callers from each other. `Custom` asks the host authenticator,
    /// passing the credential it just accepted.
    pub fn transaction_owner(
        &self,
        credential: Option<&str>,
        header_value: Option<&str>,
    ) -> Result<Option<String>, AuthError> {
        match self {
            AuthConfig::None | AuthConfig::Basic { .. } => Ok(None),
            AuthConfig::Custom(auth) => {
                let Some(credential) = credential else {
                    return Err(AuthError::Rejected);
                };
                auth.transaction_owner(credential, header_value)
            }
        }
    }

    /// The 401 challenge for this mode.
    pub fn challenge(&self) -> (String, String) {
        match self {
            AuthConfig::Custom(_) => (
                "WWW-Authenticate".to_string(),
                format!("Bearer realm=\"{REALM}\""),
            ),
            AuthConfig::None | AuthConfig::Basic { .. } => basic_challenge(),
        }
    }

    /// Turn an authentication error into its one HTTP response recipe.
    pub fn http_failure(&self, error: AuthError) -> HttpAuthFailure {
        match error {
            AuthError::Rejected => HttpAuthFailure {
                status: 401,
                message: UNAUTHORIZED_MESSAGE,
                challenge: Some(self.challenge()),
            },
            AuthError::Unavailable => HttpAuthFailure {
                status: 502,
                message: UNAVAILABLE_MESSAGE,
                challenge: None,
            },
        }
    }

    /// Validate an RFC 7617 `Authorization` header value (the full header
    /// value, e.g. `Basic dXNlcjpwYXNz`). A missing, non-Basic, or
    /// malformed header never passes under `Basic`.
    pub fn check_basic(&self, header: Option<&str>) -> bool {
        self.check_authorization(header).is_ok()
    }

    /// The listening banner's short label for the effective auth mode.
    pub fn banner_label(&self) -> &'static str {
        match self {
            AuthConfig::None => "none",
            AuthConfig::Basic { .. } => "basic",
            AuthConfig::Custom(_) => "custom",
        }
    }

    /// The principal a validated presentation has *proven* (ADR-0273).
    /// Under `Basic`, `check` has just confirmed the presented principal
    /// equals the preset user, so that user is verified identity and may
    /// be reported engine-ward. Under `None` every presentation passes
    /// unchecked, so its principal is unverified client input and must
    /// never be presented as identity.
    pub fn verified_principal(&self) -> Option<&str> {
        match self {
            AuthConfig::Basic { user, .. } => Some(user),
            // A custom owner is not an engine principal (ADR-0428).
            AuthConfig::None | AuthConfig::Custom(_) => None,
        }
    }
}

/// The token from an `Authorization: Bearer` header value, if that is
/// the scheme. The scheme is case-insensitive (RFC 7235). The value is
/// the full header value, not the header name.
pub fn parse_bearer_header(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

/// The bearer token or Basic password presented in an `Authorization`
/// header. `None` when the header is missing or neither scheme.
pub fn presented_credential(header: &str) -> Option<String> {
    parse_bearer_header(header)
        .map(str::to_owned)
        .or_else(|| parse_basic_header(header).map(|(_, password)| password))
}

/// Resolve the auth configuration (flag > env > generated token) together
/// with the startup-banner lines describing the outcome; the caller prints
/// them, so resolve stays testable. The env value is an explicit input —
/// [`env_credential`] reads the process variable at the binary's edge.
pub fn resolve(
    flag: Option<&str>,
    env: Option<&str>,
    uri: &str,
) -> Result<(AuthConfig, Vec<String>), String> {
    if let Some(v) = flag {
        return from_preset(v, "--auth");
    }
    if let Some(v) = env {
        return from_preset(v, "RELATE_AUTH");
    }
    resolve_generated(uri)
}

/// A `RELATE_*` process variable holding a plain string, read at the
/// binary's edge. A present-but-non-UTF-8 value is malformed input, not
/// "unset" — the loud-error contract must not fall through to a default
/// under an operator who believes a value is set. The one read of this
/// shape: every network surface's env fallback goes through it.
pub fn env_string(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotUnicode(v)) => {
            Err(format!("{name} is not valid UTF-8 ({} bytes)", v.len()))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
    }
}

/// The `RELATE_AUTH` process variable, read at the binary's edge. A
/// present-but-non-UTF-8 value is malformed input, not "unset" — the
/// loud-error contract must not fall through to a generated token under an
/// operator who believes a credential is set.
pub fn env_credential() -> Result<Option<String>, String> {
    env_string("RELATE_AUTH")
}

/// Parse an RFC 7617 `Authorization: Basic` header value into
/// `(user, password)`. The scheme keyword is case-insensitive per
/// RFC 7235; the base64 blob decodes to `user:password` where the first
/// colon splits the pair (the password may itself contain colons; the
/// user-id may not, per RFC 7617 §2 — a colon before the split point
/// cannot occur because the split is at the first colon). Returns `None`
/// for a missing/wrong scheme, invalid base64, invalid UTF-8, or a decoded
/// value with no colon.
pub fn parse_basic_header(header: &str) -> Option<(String, String)> {
    use base64::Engine;
    let (scheme, blob) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(blob.trim())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, password) = text.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

/// Parse a preset credential from the flag or the env, attributing errors
/// and the banner to its surface (`--auth` / `RELATE_AUTH`).
fn from_preset(value: &str, source: &str) -> Result<(AuthConfig, Vec<String>), String> {
    parse(value)
        .map(|c| with_banner(c, &format!("auth: basic (from {source})")))
        .map_err(|e| format!("{source}: {e}"))
}

/// Stage three, when no credential is preset: auto-generate. Persistent
/// URIs reuse a token persisted next to the database; `:memory:` gets a
/// fresh token every start.
fn resolve_generated(uri: &str) -> Result<(AuthConfig, Vec<String>), String> {
    let Some(path) = token_path(uri)? else {
        let (user, password) = generate();
        return Ok(fresh(user, password, None));
    };
    if let Some(reused) = read_persisted(&path)? {
        return Ok(reused);
    }
    let (user, password) = generate();
    write_token(&path, &user, &password)?;
    Ok(fresh(user, password, Some(&path)))
}

/// The credential persisted next to the database, if one exists (`None` on
/// a missing file — "no token yet"). Anything else is a loud startup error,
/// never a silent rotation: a malformed or unreadable file means an
/// operator who believes the old credentials still work.
fn read_persisted(path: &Path) -> Result<Option<(AuthConfig, Vec<String>)>, String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("token file {} unreadable: {e}", path.display())),
    };
    let (user, password) = match parse(existing.trim())
        .map_err(|e| format!("token file {} is malformed: {e}", path.display()))?
    {
        AuthConfig::Basic { user, password } => (user, password),
        AuthConfig::None => {
            return Err(format!(
                "token file {} says 'none' — a file is not how you disable auth; delete it and pass --auth none",
                path.display()
            ));
        }
        AuthConfig::Custom(_) => {
            return Err(format!(
                "token file {} is not a basic credential",
                path.display()
            ));
        }
    };
    Ok(Some((
        AuthConfig::Basic { user, password },
        vec![format!(
            "auth: basic (token reused from {})",
            path.display()
        )],
    )))
}

/// Assemble a freshly generated credential and its two banner lines;
/// `persisted_at` is the token-file path for a persistent database, `None`
/// for `:memory:`.
fn fresh(user: String, password: String, persisted_at: Option<&Path>) -> (AuthConfig, Vec<String>) {
    let banner = vec![
        format!("auth: generated token for user {user}: {password}"),
        match persisted_at {
            Some(path) => format!("auth: persisted at {} (reused on restart)", path.display()),
            None => format!(
                "auth: connect with auth=({user}, <token>) — :memory: generates a fresh token every start"
            ),
        },
    ];
    (AuthConfig::Basic { user, password }, banner)
}

/// Pair a flag/env config with its banner line; a `none` config prints the
/// same warning whatever surface set it.
fn with_banner(config: AuthConfig, source_line: &str) -> (AuthConfig, Vec<String>) {
    let line = if config == AuthConfig::None {
        "auth: none (no credentials checked — publish ports only on trusted networks)".to_string()
    } else {
        source_line.to_string()
    };
    (config, vec![line])
}

/// Constant-time equality for the credential fields. Both sides are hashed
/// to a fixed 32 bytes first: a short-circuiting `==` leaks the first
/// mismatching byte position, and any variable-length comparison — an early
/// length return or a loop scaled to the input — leaks the secret's length.
/// Fixed-length digests under a full XOR-fold leak neither.
fn ct_eq(given: Option<&str>, expected: &str) -> bool {
    let Some(given) = given else { return false };
    let a = Sha256::digest(given.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// `none` disables auth; `user:password` presets a credential. Anything
/// else is a usage error.
fn parse(value: &str) -> Result<AuthConfig, String> {
    if value == "none" {
        return Ok(AuthConfig::None);
    }
    let Some((user, password)) = value.split_once(':') else {
        return Err("expected user:password or 'none'".into());
    };
    if user.is_empty() || password.is_empty() {
        return Err("expected user:password with both sides non-empty".into());
    }
    Ok(AuthConfig::Basic {
        user: user.to_string(),
        password: password.to_string(),
    })
}

/// The token file's path for a store URI — the public owner of the
/// `<path>.auth-token` fact: `resolve`'s generated-token path consults it,
/// and tests point at the generated token file through it instead of
/// re-deriving the sibling. The file is a **sibling**, never inside the
/// database (the engine rejects a storage directory holding foreign files):
/// `<path>.auth-token` for a directory or a `.rl` bundle alike; `None` for
/// a memory URI. The classification is the engine's canonical one
/// (`relate_storage::local_uri_path`): a remote scheme is an error,
/// never a silently mis-pathed token file.
pub fn token_path(uri: &str) -> Result<Option<PathBuf>, String> {
    let Some(path) = relate_storage::local_uri_path(uri).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mut sibling = Path::new(path).as_os_str().to_os_string();
    sibling.push(".auth-token");
    Ok(Some(PathBuf::from(sibling)))
}

/// A fresh `(user, password)`: a 128-bit random token, hex — generated, not
/// derived from anything.
fn generate() -> (String, String) {
    let bytes: [u8; 16] = rand::random();
    let password: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    ("relate".to_string(), password)
}

/// Write the token file with owner-only permissions, atomically (tempfile +
/// persist), so a partial write never becomes a half token.
fn write_token(path: &Path, user: &str, password: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    // 0600 at creation — no create-then-chmod window. Off-unix the tempfile
    // default (owner-only ACL) applies.
    #[cfg(unix)]
    let staging = {
        use std::os::unix::fs::PermissionsExt;
        let mut b = tempfile::Builder::new();
        b.permissions(std::fs::Permissions::from_mode(0o600));
        b
    };
    #[cfg(not(unix))]
    let staging = tempfile::Builder::new();
    // NamedTempFile stages in the same directory under a unique random name
    // (exclusive create, no planted-symlink or stale-tmp reuse) and
    // `persist` is the atomic rename into place.
    let mut tmp = staging
        .tempfile_in(parent)
        .map_err(|e| format!("staging token for {}: {e}", path.display()))?;
    use std::io::Write;
    tmp.write_all(format!("{user}:{password}\n").as_bytes())
        .map_err(|e| format!("writing {}: {e}", tmp.path().display()))?;
    tmp.persist(path)
        .map(|_| ())
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[cfg(unix)]
    #[test]
    fn env_string_is_loud_on_non_utf8_and_quiet_when_unset() {
        use std::os::unix::ffi::OsStrExt;
        let name = "RELATE_TEST_ENV_STRING_NONUTF8";
        std::env::set_var(name, std::ffi::OsStr::from_bytes(b"\xff\xfe"));
        let err = env_string(name).unwrap_err();
        assert!(err.contains(name), "{err}");
        assert!(err.contains("not valid UTF-8"), "{err}");
        std::env::remove_var(name);
        assert_eq!(env_string(name).unwrap(), None);
    }

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn debug_redacts_basic_password() {
        let config = AuthConfig::basic("neo4j", "s3cret-value");
        let rendered = format!("{config:?}");
        assert!(rendered.contains("neo4j"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
        assert!(!rendered.contains("s3cret-value"), "{rendered}");
        assert_eq!(format!("{:?}", AuthConfig::None), "None");
    }

    #[test]
    fn parse_none_and_credential_forms() {
        assert_eq!(parse("none").unwrap(), AuthConfig::None);
        let config = parse("neo4j:secret").unwrap();
        assert_eq!(
            config,
            AuthConfig::Basic {
                user: "neo4j".into(),
                password: "secret".into()
            }
        );
        for bad in ["nocolon", ":secret", "neo4j:"] {
            assert!(parse(bad).is_err(), "{bad:?} must be a usage error");
        }
    }

    #[test]
    fn basic_checks_principal_and_credentials() {
        let config = AuthConfig::Basic {
            user: "relate".into(),
            password: "token".into(),
        };
        assert!(config.check(Some("relate"), Some("token")));
        assert!(!config.check(Some("relate"), Some("wrong")));
        assert!(!config.check(Some("other"), Some("token")));
        assert!(!config.check(None, Some("token")));
        assert!(!config.check(Some("relate"), None));
        assert!(AuthConfig::None.check(None, None));
    }

    #[test]
    fn verified_principal_is_some_only_under_basic() {
        let basic = AuthConfig::Basic {
            user: "relate".into(),
            password: "token".into(),
        };
        assert_eq!(basic.verified_principal(), Some("relate"));
        assert_eq!(
            AuthConfig::None.verified_principal(),
            None,
            "under none the presented principal is unverified and never identity"
        );
    }

    #[test]
    fn ct_eq_is_exact_regardless_of_length_or_content() {
        assert!(ct_eq(Some("token"), "token"));
        assert!(ct_eq(Some(""), ""));
        assert!(ct_eq(Some("pässwörd!"), "pässwörd!"));
        for (given, expected) in [
            ("tok", "token"),    // shorter prefix
            ("tokens", "token"), // longer
            ("token", "tokEn"),  // same length, different content
            ("", "token"),       // empty side
            ("pässwörd!", "password!"),
        ] {
            assert!(
                !ct_eq(Some(given), expected),
                "{given:?} must not equal {expected:?}"
            );
        }
        assert!(!ct_eq(None, "token"));
    }

    #[test]
    fn parse_basic_header_decodes_rfc7617_form() {
        // RFC 7617's own example: Aladdin:open sesame.
        assert_eq!(
            parse_basic_header("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="),
            Some(("Aladdin".to_string(), "open sesame".to_string()))
        );
        // Case-insensitive scheme (RFC 7235); password may contain colons.
        assert_eq!(
            parse_basic_header(&format!("bAsIc {}", b64("u:p:a:s:s"))),
            Some(("u".to_string(), "p:a:s:s".to_string()))
        );
    }

    #[test]
    fn parse_bearer_header_accepts_any_scheme_case() {
        assert_eq!(parse_bearer_header("Bearer tok"), Some("tok"));
        assert_eq!(parse_bearer_header("BEARER tok"), Some("tok"));
        assert_eq!(parse_bearer_header("bEaReR tok"), Some("tok"));
        assert_eq!(parse_bearer_header("Bearer"), None);
        assert_eq!(parse_bearer_header("Basic dG9r"), None);
    }

    #[test]
    fn parse_basic_header_rejects_malformed_values() {
        for bad in [
            "Bearer dG9rZW4=",     // wrong scheme
            "Basic",               // no blob
            "Basic !!!not-base64", // invalid base64
            "Basic bm9jb2xvbg==",  // "nocolon" — decoded value has no colon
            "Basic aGVsbG8g+d==",  // invalid UTF-8 after decode
        ] {
            assert_eq!(parse_basic_header(bad), None, "{bad:?} must not parse");
        }
    }

    #[test]
    fn check_basic_validates_the_authorization_header() {
        let config = AuthConfig::basic("relate", "token");
        let good = format!("Basic {}", b64("relate:token"));
        assert!(config.check_basic(Some(&good)));
        assert!(!config.check_basic(Some(&format!("Basic {}", b64("relate:wrong")))));
        assert!(!config.check_basic(Some(&format!("Basic {}", b64("other:token")))));
        assert!(!config.check_basic(Some("Basic !!!")));
        assert!(!config.check_basic(None));
        assert!(
            AuthConfig::None.check_basic(None),
            "under none the header is ignored entirely"
        );
    }

    #[test]
    fn generated_config_validates_its_own_printed_token() {
        let dir = tempfile::tempdir().unwrap();
        let uri = dir.path().join("graph").to_str().unwrap().to_string();
        let (config, _) = resolve(None, None, &uri).unwrap();
        let AuthConfig::Basic { user, password } = &config else {
            panic!("a generated credential is basic");
        };
        assert!(config.check(Some(user), Some(password)));
        assert!(config.check_basic(Some(&format!(
            "Basic {}",
            b64(&format!("{user}:{password}"))
        ))));
    }

    #[test]
    fn token_path_forms() {
        assert_eq!(token_path(":memory:").unwrap(), None);
        assert_eq!(
            token_path("/data/graph").unwrap(),
            Some(PathBuf::from("/data/graph.auth-token"))
        );
        assert_eq!(
            token_path("/data/graph.rl").unwrap(),
            Some(PathBuf::from("/data/graph.rl.auth-token"))
        );
        assert_eq!(
            token_path("file:///data/graph").unwrap(),
            Some(PathBuf::from("/data/graph.auth-token"))
        );
        // A remote scheme is the engine's canonical error, not a mis-pathed
        // token file.
        assert!(token_path("s3://bucket/graph")
            .unwrap_err()
            .contains("s3://"));
    }

    #[test]
    fn generated_tokens_persist_and_reuse_for_persistent_uris() {
        let dir = tempfile::tempdir().unwrap();
        let uri = dir.path().join("graph").to_str().unwrap().to_string();
        let (first, _) = resolve(None, None, &uri).unwrap();
        let path = token_path(&uri).unwrap().unwrap();
        assert!(path.exists());
        let mode = std::fs::metadata(&path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(mode.mode() & 0o777, 0o600);
        }
        let (second, _) = resolve(None, None, &uri).unwrap();
        assert_eq!(first, second, "a persisted token is reused, not rotated");
    }

    #[test]
    fn memory_generates_fresh_each_time() {
        let (a, _) = resolve(None, None, ":memory:").unwrap();
        let (b, _) = resolve(None, None, ":memory:").unwrap();
        assert_ne!(a, b, ":memory: rotates its token on every start");
    }

    #[test]
    fn the_flag_wins_over_the_env_and_neither_touches_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let uri = dir.path().join("graph").to_str().unwrap().to_string();
        let (config, banner) = resolve(Some("flag:cred"), Some("env:cred"), &uri).unwrap();
        assert_eq!(
            config,
            AuthConfig::Basic {
                user: "flag".into(),
                password: "cred".into()
            }
        );
        assert_eq!(banner, vec!["auth: basic (from --auth)".to_string()]);
        let (config, banner) = resolve(None, Some("env:cred"), &uri).unwrap();
        assert_eq!(
            config,
            AuthConfig::Basic {
                user: "env".into(),
                password: "cred".into()
            }
        );
        assert_eq!(banner, vec!["auth: basic (from RELATE_AUTH)".to_string()]);
        assert!(
            !token_path(&uri).unwrap().unwrap().exists(),
            "a preset credential writes no token file"
        );
    }

    #[test]
    fn malformed_persisted_token_is_a_loud_error() {
        let dir = tempfile::tempdir().unwrap();
        let uri = dir.path().join("graph").to_str().unwrap().to_string();
        let path = token_path(&uri).unwrap().unwrap();
        std::fs::write(&path, "not-a-credential\n").unwrap();
        let err = resolve(None, None, &uri).unwrap_err();
        assert!(err.contains("malformed"), "{err}");
        std::fs::write(&path, "none\n").unwrap();
        let err = resolve(None, None, &uri).unwrap_err();
        assert!(err.contains("'none'"), "{err}");
    }
}
