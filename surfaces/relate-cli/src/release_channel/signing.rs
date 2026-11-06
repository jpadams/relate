//! The one signing root both channel flows verify against (ADR-0115,
//! ADR-0431 §1): the release signing public key pinned in source (never
//! fetched), the SSHSIG namespace, and the pure verification. `upgrade`
//! authenticates `SHA256SUMS` with it; `extension install` authenticates
//! the channel index `extensions.json` with the same key — first-party is
//! "published by this repo's release channel", one root, no second
//! verification path (ADR-0431's rejected alternative).
//!
//! The failure is typed, not texted: each flow renders its own pinned
//! wording (the caller names the artifact that failed to verify), so the
//! shared root cannot pick a command's message.

/// Release signing public key, pinned in source and never fetched from the
/// release (ADR-0115). Kept byte-identical to
/// `distribution/release-signing-key.pub` and `install.sh` (the test below
/// pins that).
pub(crate) const RELEASE_SIGNING_PUBLIC_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHgv/GHY5mtv/HopH/J/T7KrTT7eOVFEG2ueT4iRKwbK relate-release";

/// SSHSIG namespace release-channel documents are signed under (ADR-0115):
/// `SHA256SUMS` for `upgrade`, `extensions.json` for the extension channel
/// (ADR-0431 §1) — the same key and namespace, different documents.
pub(crate) const SIGNATURE_NAMESPACE: &str = "relate-release";

/// A signature verification failure, typed so each flow renders its own
/// pinned text naming the artifact it verified (ADR-0431: every failure
/// path has a pinned, tested text).
#[derive(Debug)]
pub(crate) enum SigningError {
    /// The pinned key itself failed to parse (a build- or rotation-time
    /// invariant failure).
    InvalidKey(ssh_key::Error),
    /// The signature blob is not a valid SSHSIG.
    MalformedSignature(ssh_key::Error),
    /// The signature did not verify against the pinned key: wrong key,
    /// tampered document, or wrong namespace.
    VerificationFailed(ssh_key::Error),
}

/// Verify the OpenSSH SSHSIG `sig_pem` over the exact `message` bytes
/// against the pinned [`RELEASE_SIGNING_PUBLIC_KEY`] in
/// [`SIGNATURE_NAMESPACE`] (ADR-0115). Pure: no network, so the whole
/// authentication path of both flows is unit-testable with fixture keys
/// (the TEST_SIGNER pattern, ADR-0431's consequences). The hash algorithm
/// is carried inside the signature (`ssh-keygen`'s default SHA-512).
pub(crate) fn verify(message: &[u8], sig_pem: &[u8]) -> Result<(), SigningError> {
    verify_with_key(message, sig_pem, RELEASE_SIGNING_PUBLIC_KEY)
}

/// [`verify`] against an explicit `public_key_line`: the shared core the
/// pinned-key entry point, the fixture-key unit tests, and the install
/// flow's fixture-index driver (ADR-0431's consequences: signature checks
/// with fixture keys, the TEST_SIGNER pattern — the pinned release key
/// never verifies a fixture-key signature, so the channel's happy path
/// pins the verification shape here and the wrong-key refusal in the e2e)
/// all run, so the wrong-key refusal (a signature by a different signer)
/// is testable without a second pinned constant.
pub(crate) fn verify_with_key(
    message: &[u8],
    sig_pem: &[u8],
    public_key_line: &str,
) -> Result<(), SigningError> {
    let public_key =
        ssh_key::PublicKey::from_openssh(public_key_line).map_err(SigningError::InvalidKey)?;
    let signature = ssh_key::SshSig::from_pem(sig_pem).map_err(SigningError::MalformedSignature)?;
    public_key
        .verify(SIGNATURE_NAMESPACE, message, &signature)
        .map_err(SigningError::VerificationFailed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only Ed25519 keypairs, generated with `ssh-keygen -t ed25519`
    // and never the release key. Only the public halves appear here; the
    // private halves are not committed, and no test opens a socket.
    const TEST_SIGNER_PUB: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGlvgULOpHz9PDoNJcz642z8HKOouS/I91vfscNYYvvx relate-unit-fixture";
    const TEST_OTHER_PUB: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIL7F2GGp2tqieYscxnKMP5ez15W5Mb12+QC1Oc4QmBz relate-test-other";

    /// The exact document bytes the SSHSIG below was computed over.
    const TEST_MESSAGE: &str = concat!(
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  relate-darwin-arm64\n",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  relate-linux-amd64\n",
    );

    /// `ssh-keygen -Y sign -f <signer> -n relate-release SHA256SUMS`.
    const TEST_SIG: &str = "\
-----BEGIN SSH SIGNATURE-----
U1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgaW+BQs6kfP08Og0lzPrjbPwco6
i5L8j3W9+xw1hi+/EAAAAOcmVsYXRlLXJlbGVhc2UAAAAAAAAABnNoYTUxMgAAAFMAAAAL
c3NoLWVkMjU1MTkAAABA+tbf0+tDN4p0Ths2aCt0mp2PmGhpdeGWROyMax9v3Q2rFQ5+iN
JRwH+6xkr9RJgEC9M4rIcfJg6QHZ8uRRXDDQ==
-----END SSH SIGNATURE-----
";

    /// The fixture signature verifies against its fixture signer's key;
    /// `verify()` (the pinned-key entry point) is exercised against the
    /// real key end to end by `e2e_upgrade`'s committed `fixtures/release/`
    /// pair.
    #[test]
    fn verify_accepts_a_valid_signature() {
        verify_with_key(
            TEST_MESSAGE.as_bytes(),
            TEST_SIG.as_bytes(),
            TEST_SIGNER_PUB,
        )
        .unwrap();
    }

    #[test]
    fn verify_rejects_a_tampered_message() {
        let tampered = TEST_MESSAGE.replace(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert_ne!(tampered, TEST_MESSAGE);
        assert!(matches!(
            verify(tampered.as_bytes(), TEST_SIG.as_bytes()),
            Err(SigningError::VerificationFailed(_))
        ));
    }

    #[test]
    fn verify_rejects_the_wrong_key() {
        // A signature by a different signer, verified against a key that is
        // not the signer's: the pinned-root refusal by value (wrong key,
        // tampered document, wrong namespace all land here).
        assert!(matches!(
            verify_with_key(TEST_MESSAGE.as_bytes(), TEST_SIG.as_bytes(), TEST_OTHER_PUB),
            Err(SigningError::VerificationFailed(_))
        ));
    }

    #[test]
    fn verify_rejects_a_malformed_signature() {
        for bad in [
            b"not a pem".as_slice(),
            b"",
            b"-----BEGIN SSH SIGNATURE-----\nzzzz\n-----END SSH SIGNATURE-----\n",
        ] {
            assert!(
                matches!(
                    verify(TEST_MESSAGE.as_bytes(), bad),
                    Err(SigningError::MalformedSignature(_))
                ),
                "{bad:?} must fail as a malformed SSHSIG"
            );
        }
    }

    #[test]
    fn verify_rejects_an_unparseable_key() {
        assert!(matches!(
            verify_with_key(TEST_MESSAGE.as_bytes(), TEST_SIG.as_bytes(), "not a key"),
            Err(SigningError::InvalidKey(_))
        ));
    }

    #[test]
    fn the_pinned_release_key_parses() {
        // The constant pinned in `verify` must be a valid OpenSSH key.
        assert!(ssh_key::PublicKey::from_openssh(RELEASE_SIGNING_PUBLIC_KEY).is_ok());
    }

    #[test]
    fn the_pinned_release_key_matches_the_canonical_file() {
        // Key rotation must update all pinned copies together: this
        // constant, `distribution/release-signing-key.pub` (canonical), and
        // install.sh's SIGNING_KEY (guarded by its own test).
        let canonical = include_str!("../../../../distribution/release-signing-key.pub");
        assert_eq!(
            RELEASE_SIGNING_PUBLIC_KEY,
            canonical.trim_end(),
            "RELEASE_SIGNING_PUBLIC_KEY drifted from distribution/release-signing-key.pub"
        );
    }
}
