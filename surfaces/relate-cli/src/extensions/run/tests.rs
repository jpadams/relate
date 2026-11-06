//! The `run` verb mount's behavior tests (LABS-552), in the dir-module
//! pattern the extension tree's own modules use: the parse decisions
//! over fixture registries (the dispatch core, the pinned discovery
//! error, the missing-name listing, the action-scope error data), and
//! the verb page's installed-actions section over the same fixtures.
//! The exit-2 renderings and the real-binary flow are the e2e suite's
//! (`tests/e2e_run.rs`) — `usage_error` exits, so no unit test can call
//! it.

use super::*;
use crate::extensions::testkit::{
    extension, install_package, manifest_for, registry_with, EVERY_KIND_FLAGS,
};

/// The empty registry — the fresh-install state (`$XDG_CONFIG_HOME`
/// resolving to neither base).
fn empty_registry() -> ActionRegistry {
    ActionRegistry::load_with(&crate::telemetry::env_of(&[])).unwrap()
}

/// Two visible extensions under one tempdir XDG, in the walk's sorted
/// order — the multi-action listing fixture.
fn installed_pair() -> (ActionRegistry, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["beta", "alpha"] {
        install_package(&tmp, name, &manifest_for(name, "", ""));
    }
    let xdg = tmp.path().to_string_lossy().into_owned();
    let pairs = [("XDG_CONFIG_HOME", xdg.as_str())];
    let registry = ActionRegistry::load_with(&crate::telemetry::env_of(&pairs)).unwrap();
    (registry, tmp)
}

// ---------------------------------------------------------------------
// The dispatch core (every platform)
// ---------------------------------------------------------------------

#[test]
fn a_bare_run_lists_the_installed_actions_in_its_missing_name_error() {
    // ADR-0429 §1: the dynamic installed-actions section lives on the
    // run usage error — the missing-name refusal names what is
    // installed.
    let (registry, _tmp) = registry_with("greet", EVERY_KIND_FLAGS, "");
    let err = parse_run_word(&[], &registry).unwrap_err();
    let RunUsageError::Verb(err) = err else {
        panic!("the bare run is the verb's own refusal: {err:?}");
    };
    assert_eq!(
        err.to_string(),
        "run requires an action name (installed: greet)"
    );
}

#[test]
fn the_ship_with_zero_state_names_the_discovery_path() {
    // The shipped state (ADR-0429 §7): zero installed extensions, and
    // the missing-name error says so while naming the first-party path.
    let registry = empty_registry();
    let err = parse_run_word(&[], &registry).unwrap_err();
    let RunUsageError::Verb(err) = err else {
        panic!("the bare run is the verb's own refusal: {err:?}");
    };
    assert_eq!(
        err.to_string(),
        "run requires an action name (no actions installed; \
          relate extension install <name> adds one)"
    );
}

/// A hidden extension still parses and runs but appears in no listing
/// (ADR-0429 §2): the missing-name error counts only the visible rows,
/// while `run <name>` still resolves and parses the hidden action.
#[test]
fn a_hidden_extension_is_unlisted_but_still_runs() {
    let (registry, _tmp) = registry_with("greet", EVERY_KIND_FLAGS, r#","hidden": true"#);
    let err = parse_run_word(&[], &registry).unwrap_err();
    let RunUsageError::Verb(err) = err else {
        panic!("the bare run is the verb's own refusal: {err:?}");
    };
    assert_eq!(
        err.to_string(),
        "run requires an action name (no actions installed; \
          relate extension install <name> adds one)"
    );
    let cmd = parse_run_word(&crate::args(&["greet"]), &registry).unwrap();
    assert_eq!(cmd.entry.mount_name(), "greet");
}

/// ADR-0429 §7's pinned discovery text (LABS-552 §4): an unresolvable
/// name fails closed with the exact wording — never a fetch, never a
/// suggestion.
#[test]
fn an_unknown_name_fails_closed_with_the_pinned_discovery_text() {
    let (registry, _tmp) = registry_with("greet", EVERY_KIND_FLAGS, "");
    let err = parse_run_word(&crate::args(&["bogus"]), &registry).unwrap_err();
    let RunUsageError::Verb(err) = err else {
        panic!("the discovery refusal is the verb's own: {err:?}");
    };
    assert_eq!(
        err.to_string(),
        "no installed action 'bogus' — first-party extension available: \
          relate extension install bogus"
    );
    // The refusal is a plain message: no rejected argument, so no
    // did-you-mean can attach to it.
    assert_eq!(err.suggestion(&["--database"], &[]), None);
}

/// A flag where the name belongs is an unexpected argument at the
/// verb's scope — the verb's own grammar takes no flag before the
/// action name, and the shared renderer suggests from the run spec's
/// static flags.
#[test]
fn a_flag_where_the_name_belongs_is_an_unexpected_argument() {
    let registry = empty_registry();
    let err = parse_run_word(&crate::args(&["--bogus"]), &registry).unwrap_err();
    let RunUsageError::Verb(err) = err else {
        panic!("a flag at the name position is the verb's own refusal: {err:?}");
    };
    assert_eq!(err.to_string(), "unexpected argument to run: --bogus");
}

#[test]
fn an_installed_action_parses_through_the_hosts_grammar() {
    let (registry, _tmp) = registry_with(
        "greet",
        EVERY_KIND_FLAGS,
        r#","positional": {"name": "DIR"}"#,
    );
    let cmd = parse_run_word(
        &crate::args(&["greet", "DIR", "--message", "hi", "-d", "./graph"]),
        &registry,
    )
    .unwrap();
    assert_eq!(cmd.entry.mount_name(), "greet");
    // The host's parse: the flag's value, the positional, and the
    // host-resolved store target (ADR-0429 §5, ADR-0430 §6).
    assert_eq!(
        cmd.invocation.envelope.get("message"),
        Some(&serde_json::json!("hi"))
    );
    assert_eq!(
        cmd.invocation.envelope.get("DIR"),
        Some(&serde_json::json!("DIR"))
    );
    assert_eq!(cmd.invocation.db.as_deref(), Some("./graph"));
}

/// An action-scope refusal carries the action's OWN rendering data
/// (ADR-0429 §5): its usage line, its did-you-mean candidates, and the
/// hint naming the action — all manifest data, all sanitized where
/// composed.
#[test]
fn an_action_scope_refusal_carries_the_actions_own_usage_hint_and_candidates() {
    let (registry, _tmp) = registry_with(
        "greet",
        EVERY_KIND_FLAGS,
        r#","positional": {"name": "DIR"}"#,
    );
    let err = parse_run_word(&crate::args(&["greet", "--mesage", "hi"]), &registry).unwrap_err();
    let RunUsageError::Action(refusal) = err else {
        panic!("the action's parse refusal renders in the action's scope: {err:?}");
    };
    let ActionRefusal {
        error,
        usage,
        hint,
        candidates,
    } = *refusal;
    assert_eq!(
        error.to_string(),
        "unexpected argument to run greet: --mesage"
    );
    let entry = extension(&registry, "greet");
    assert_eq!(usage, action_usage(entry));
    assert_eq!(
        usage,
        "Usage: relate run greet [DIR] [--message VALUE] [--count N] \
          [--shout] [--tag VALUE]"
    );
    assert_eq!(hint, "Run 'relate run greet --help' for more information.");
    assert_eq!(
        candidates,
        [
            "--message",
            "--count",
            "--shout",
            "--tag",
            "-d",
            "--database"
        ]
    );
    // The suggestion the renderer will print: the same candidate list
    // the grammar accepts (ADR-0230's derive-from-the-one-table rule).
    let refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    assert_eq!(error.suggestion(&refs, &[]), Some("--message"));
}

// ---------------------------------------------------------------------
// The help pre-scan (every platform)
// ---------------------------------------------------------------------

/// A name that resolves prints the action's own page — rendered from
/// the same registry load that resolved it; anything else prints the
/// verb's (help wins, nothing is validated).
#[test]
fn the_help_page_is_the_resolved_actions_own() {
    let (registry, _tmp) = registry_with("greet", EVERY_KIND_FLAGS, "");
    let entry = extension(&registry, "greet");
    assert_eq!(
        help_page("greet", &registry),
        ParsedRun::ActionHelp(action_help(entry))
    );
    assert_eq!(help_page("bogus", &registry), ParsedRun::VerbHelp);
}

/// A hidden extension's help page is reachable by name (hidden hides
/// listings, not the action — ADR-0429 §2).
#[test]
fn a_hidden_extensions_help_page_resolves_by_name() {
    let (registry, _tmp) = registry_with("greet", EVERY_KIND_FLAGS, r#","hidden": true"#);
    assert!(matches!(
        help_page("greet", &registry),
        ParsedRun::ActionHelp(_)
    ));
}

/// A manifest fragment cannot forge structure into an action's page
/// (the host's own rule, reached through the verb: the page the
/// pre-scan prints is the host's sanitized renderer).
#[test]
fn a_manifest_fragment_cannot_forge_structure_into_the_actions_page() {
    let flags = r#"{"kind": "string", "name": "banner", "default": "a\tb"}"#;
    let (registry, _tmp) = registry_with("noisy", flags, "");
    let ParsedRun::ActionHelp(page) = help_page("noisy", &registry) else {
        panic!("the fixture must resolve: the help page is the action's own");
    };
    assert!(
        !page.contains('\t') && !page.contains('\u{1b}'),
        "control characters reached the page raw:\n{page:?}"
    );
}

// ---------------------------------------------------------------------
// The listing (every platform)
// ---------------------------------------------------------------------

/// The verb's listing is the registry's extension rows minus the
/// hidden ones (ADR-0429 §2) — the one policy the help page and the
/// missing-name error read, pinned here as data: sorted walk order,
/// hidden rows excluded.
#[test]
fn the_listing_is_the_visible_extension_rows_in_walk_order() {
    let (registry, _tmp) = installed_pair();
    let listed: Vec<&str> = installed_actions(&registry)
        .map(|entry| entry.mount_name())
        .collect();
    assert_eq!(listed, ["alpha", "beta"]);
    let (hidden, _tmp) = registry_with("shy", "", r#","hidden": true"#);
    let listed: Vec<&str> = installed_actions(&hidden)
        .map(|entry| entry.mount_name())
        .collect();
    assert_eq!(listed, Vec::<&str>::new());
}
