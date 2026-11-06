use super::*;

// --- the `--url` tier's transport gate (ADR-0431 §6, the trust-act fix) ---

/// The third-party tier has no signature to verify, so https is the
/// act's one leg of integrity: every non-https URL refuses with the
/// authored usage error before any fetch, and nothing lands.
#[test]
fn the_url_tier_refuses_a_non_https_url_with_the_authored_error() {
    let world = World::new();
    let staged = staging_dir(&world);
    for url in [
        "http://example.test/greet.tar.gz",
        "ftp://example.test/greet.tar.gz",
        "file:///tmp/greet.tar.gz",
        "example.test/greet.tar.gz",
    ] {
        let err = install_from_url("greet", url, &staged)
            .expect_err("a non-https URL refuses before any fetch");
        assert_eq!(err.exit_code(), 2, "{url}: {}", err.stderr_message());
        assert!(
            err.stderr_message().contains(
                "--url accepts only https URLs (or plain http to the loopback interface)"
            ),
            "{url}: {}",
            err.stderr_message()
        );
        assert!(
            err.stderr_message().contains(url),
            "{url}: {}",
            err.stderr_message()
        );
    }
    // The gate fires before any fetch: staging never saw a member.
    assert!(std::fs::read_dir(&staged).unwrap().next().is_none());
}

/// The tier's download-failure wrap: an https URL nothing serves
/// (loopback port 1, the harness's own refused endpoint port) fails
/// closed naming the URL — the wrap the fetch errors ride through.
#[test]
fn the_url_tier_wraps_a_failed_download_naming_the_url() {
    let world = World::new();
    let staged = staging_dir(&world);
    let err = install_from_url("greet", "https://127.0.0.1:1/gone.tar.gz", &staged)
        .expect_err("an https URL nothing serves fails closed");
    assert_eq!(err.exit_code(), 1, "{}", err.stderr_message());
    assert!(
        err.stderr_message().contains(
            "could not download the package tarball from https://127.0.0.1:1/gone.tar.gz"
        ),
        "{}",
        err.stderr_message()
    );
    assert!(std::fs::read_dir(&staged).unwrap().next().is_none());
}
