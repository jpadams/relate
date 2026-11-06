# Release manifest fixture

`SHA256SUMS` is a release manifest in the pre-ADR-0457 shape (no signed
version header) under the current asset names; `SHA256SUMS.sig` is an SSHSIG
over those exact bytes by the committed fixture signer
(`test-signing-key.pub`) in the `relate-release` namespace — deliberately
**not** the pinned release key.

The `upgrade` download-route e2e tests (`tests/e2e_upgrade.rs`) serve these
bytes so the whole authentication path runs offline: the route fetches the
manifest and its signature, and the pinned release key refuses a document
this channel did not sign **before** the binary download.

The pair used to be the real v0.12.5 release artifacts, signed by the release
key. ADR-0517 §6 changed the SSHSIG namespace to `relate-release`, which a
pre-cutover signature cannot carry, so no pre-cutover artifact can verify in
Relate any more — the swan-song 0.16.1 release is verified by old
binaries under the old identity, and nothing published afterwards needs
that. Restore a real-artifact positive fixture once the first Relate release
exists (it will be signed by the release key in the new namespace):

    gh release download vX.Y.Z -R relatedb/relate -p SHA256SUMS -p SHA256SUMS.sig
