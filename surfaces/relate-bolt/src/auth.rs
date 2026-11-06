//! Bolt wire adaptation of the shared auth mechanism (ADR-0232). All
//! security logic — resolution order, generated-token persistence (0600
//! sibling), constant-time comparison, the `none` opt-out — lives in
//! [`relate_auth`]; this module only re-exports it and adapts
//! [`AuthConfig::check`] to the Bolt HELLO map's wire shape: the shared
//! `check` is scheme-neutral, while Bolt's HELLO carries an explicit
//! `scheme` field that must be `basic` (ADR-0126).

pub use relate_auth::{env_credential, resolve, AuthConfig};

/// Validate a Bolt HELLO auth map against `config`. The HELLO map is
/// packstream, not an HTTP header, so the caller extracts the three
/// string fields; this shim adds Bolt's scheme gate (only `basic` names
/// a credential the server can check) around the shared pair comparison.
/// The bitwise `&` keeps the credential comparison running on a wrong
/// scheme: skipping it would make the skipped work itself a timing
/// signal.
pub(crate) fn check_hello(
    config: &AuthConfig,
    scheme: Option<&str>,
    principal: Option<&str>,
    credentials: Option<&str>,
) -> bool {
    match config {
        AuthConfig::None => true,
        AuthConfig::Basic { .. } => {
            matches!(scheme, Some("basic")) & config.check(principal, credentials)
        }
        AuthConfig::Custom(auth) => {
            let scheme_ok = matches!(scheme, Some("basic"));
            let credential_ok = credentials.is_some_and(|c| auth.verify_credential(c).is_ok());
            scheme_ok & credential_ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_requires_the_basic_scheme_under_basic_auth() {
        let config = AuthConfig::basic("relate", "token");
        assert!(check_hello(
            &config,
            Some("basic"),
            Some("relate"),
            Some("token")
        ));
        for scheme in [None, Some("kerberos"), Some("Basic"), Some("none")] {
            assert!(
                !check_hello(&config, scheme, Some("relate"), Some("token")),
                "scheme {scheme:?} must not pass under basic auth"
            );
        }
        assert!(!check_hello(
            &config,
            Some("basic"),
            Some("relate"),
            Some("wrong")
        ));
        assert!(check_hello(&AuthConfig::None, None, None, None));
    }
}
