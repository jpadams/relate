#!/bin/sh
# Offline, hermetic test harness for distribution/install.sh.
#
# The installer is run with HOME, TMPDIR and PATH pointed at a sandbox whose
# stub-bin/ shadows `curl` (always) and `uname` (to fix the OS/arch under
# test). The stub curl never touches the network: it logs its argv (one arg
# per line, calls separated by an =CALL= marker) and serves fixtures keyed on
# the request URL, with failure modes selected via STUB_MODE. The harness
# writes only inside the sandbox and removes it on exit, failure included.
# The fixture/stub factories and the per-case run harness live in
# scripts/tests/lib/install_sh_harness.sh; this file holds the drift guards
# and the cases.
#
# Output and exit contract: scripts/tests/lib/tap.sh.

set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
INSTALLER="$ROOT/distribution/install.sh"

# Shared script helpers (sha256_of, for the SHA256SUMS fixture below;
# common.sh's bodies are POSIX sh, so `.` works here).
. "$ROOT/scripts/lib/common.sh"
# The shared TAP harness (LABS-622): tap_check/tap_skip/tap_done, the
# counters, output format, and sandbox policy spelled once.
. "$ROOT/scripts/tests/lib/tap.sh"

tap_sandbox relate-install-sh-test

stubbin="$sandbox/stub-bin"
fixtures="$sandbox/fixtures"
itmp="$sandbox/itmp"
mkdir -p "$stubbin" "$fixtures" "$itmp"

# The harness machinery (fixtures, stubs, run helpers) comes from the
# sourced lib; the case script below owns only the guards and assertions.
. "$ROOT/scripts/tests/lib/install_sh_harness.sh"

# --- 0. Drift guards ------------------------------------------------------------

# The pinned SIGNING_KEY in install.sh must match the canonical public key
# committed at distribution/release-signing-key.pub; a release that rotates one
# without the other breaks verification for every user.
pinned_key=$(sed -n "s/^SIGNING_KEY='\\(.*\\)'\$/\\1/p" "$INSTALLER")
pub_key=$(sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' \
	"$ROOT/distribution/release-signing-key.pub")
[ -n "$pinned_key" ]
tap_check "drift guard: install.sh carries a pinned SIGNING_KEY assignment" $?
[ "$pinned_key" = "$pub_key" ]
tap_check "drift guard: pinned SIGNING_KEY matches distribution/release-signing-key.pub" $?

# The ADR-0457 wire contract exists in four spellings: the release producer's
# printf, manifest.rs's constants, install.sh's copy, and this harness's
# fixture writer. The prefix is the grammar itself (one space after the colon
# is load-bearing), so all four must carry the identical literal; the ceiling
# must agree too, or the two parsers accept different legacy windows.
manifest_rs="$ROOT/surfaces/relate-cli/src/upgrade_cmd/manifest.rs"
installer_ceiling=$(sed -n 's/^LEGACY_MANIFEST_CEILING=//p' "$INSTALLER")
rust_ceiling=$(sed -n 's/^const LEGACY_MANIFEST_CEILING: (u64, u64, u64) = (\([0-9][0-9]*\), \([0-9][0-9]*\), \([0-9][0-9]*\));$/\1.\2.\3/p' \
	"$manifest_rs")
[ -n "$installer_ceiling" ]
tap_check "drift guard: install.sh carries a LEGACY_MANIFEST_CEILING assignment" $?
[ -n "$rust_ceiling" ]
tap_check "drift guard: manifest.rs carries the LEGACY_MANIFEST_CEILING constant" $?
[ "$installer_ceiling" = "$rust_ceiling" ]
tap_check "drift guard: install.sh ceiling matches manifest.rs (ADR-0457)" $?

# Prefix parity: the producer's printf, install.sh's assignment and the
# harness fixture writer's constant must all equal the literal manifest.rs
# carries, and install.sh's case pattern must match the header through that
# one variable rather than a re-spelled literal.
installer_prefix=$(sed -n "s/^SUMS_VERSION_PREFIX='\\(.*\\)'\$/\\1/p" "$INSTALLER")
rust_prefix=$(sed -n 's/^const SUMS_VERSION_PREFIX: &str = "\(.*\)";$/\1/p' \
	"$manifest_rs")
release_prefix=$(sed -n "s/^[[:space:]]*printf '\(.*\)%s\\\\n'.*> SHA256SUMS\$/\1/p" \
	"$ROOT/.github/workflows/release.yml")
[ -n "$installer_prefix" ]
tap_check "drift guard: install.sh carries the SUMS_VERSION_PREFIX constant" $?
[ -n "$rust_prefix" ]
tap_check "drift guard: manifest.rs carries the SUMS_VERSION_PREFIX constant" $?
[ -n "$release_prefix" ]
tap_check "drift guard: release workflow carries the SUMS_VERSION_PREFIX literal" $?
[ "$installer_prefix" = "$rust_prefix" ]
tap_check "drift guard: install.sh header prefix matches manifest.rs (ADR-0457)" $?
[ "$release_prefix" = "$rust_prefix" ]
tap_check "drift guard: release workflow header prefix matches manifest.rs (ADR-0457)" $?
[ "$SUMS_VERSION_PREFIX" = "$rust_prefix" ]
tap_check "drift guard: harness fixture prefix matches manifest.rs (ADR-0457)" $?
grep -qF 'case "$first_line" in' "$INSTALLER" &&
	grep -qF '"$SUMS_VERSION_PREFIX"*)' "$INSTALLER"
tap_check "drift guard: install.sh matches the header through the shared prefix" $?

# The legacy-tag fixture (built below) derives from the ceiling the guard
# just extracted, so a constant change cannot leave the fixture on a stale
# hardcoded tag.
LEGACY_MANIFEST_TAG="v$rust_ceiling"

# Build the fixtures and PATH stubs only now that their legacy-tag input is
# pinned to the guarded ceiling.
install_sh_fixtures
install_sh_stubs

# --- 1. OS/arch mapping -------------------------------------------------------

reset_case
install_run osarch-darwin
[ "$RUN_STATUS" -eq 0 ]
tap_check "Darwin/arm64: installer exits 0" $?
grep -q "releases/assets/$ASSET_ID_DARWIN\$" "$RUN_LOG"
tap_check "Darwin/arm64: downloads the relate-darwin-arm64 asset id" $?
! grep -q "releases/assets/$ASSET_ID_LINUX\$" "$RUN_LOG"
tap_check "Darwin/arm64: does not download the linux-amd64 asset id" $?
cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-darwin-arm64"
tap_check "Darwin/arm64: installed content is the darwin-arm64 fixture" $?

reset_case
STUB_UNAME_S=Linux
STUB_UNAME_M=x86_64
install_run osarch-linux
[ "$RUN_STATUS" -eq 0 ]
tap_check "Linux/x86_64: installer exits 0" $?
grep -q "releases/assets/$ASSET_ID_LINUX\$" "$RUN_LOG"
tap_check "Linux/x86_64: downloads the relate-linux-amd64 asset id" $?
cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-linux-amd64"
tap_check "Linux/x86_64: installed content is the linux-amd64 fixture" $?

reset_case
STUB_UNAME_S=FreeBSD
install_run osarch-freebsd
[ "$RUN_STATUS" -ne 0 ]
tap_check "unsupported OS (FreeBSD): installer dies non-zero" $?
grep -q "unsupported OS" "$RUN_ERR"
tap_check "unsupported OS (FreeBSD): error mentions unsupported OS" $?

reset_case
STUB_UNAME_M=i386
install_run osarch-i386
[ "$RUN_STATUS" -ne 0 ]
tap_check "unsupported arch (i386): installer dies non-zero" $?
grep -q "unsupported architecture" "$RUN_ERR"
tap_check "unsupported arch (i386): error mentions unsupported architecture" $?

# --- 2. GITHUB_TOKEN handling -------------------------------------------------

reset_case
GITHUB_TOKEN=test-token
install_run token-set
[ "$RUN_STATUS" -eq 0 ]
tap_check "token set: installer exits 0" $?
every_call_has "Authorization: Bearer test-token"
tap_check "token set: every curl call carries Authorization: Bearer test-token" $?
asset_calls_have "Accept: application/octet-stream"
tap_check "token set: asset downloads carry Accept: application/octet-stream" $?
[ "$(asset_call_count)" -eq 3 ]
tap_check "token set: exactly three asset downloads (binary + SHA256SUMS + SHA256SUMS.sig)" $?

reset_case
install_run token-unset
[ "$RUN_STATUS" -eq 0 ]
tap_check "token unset: installer exits 0" $?
! grep -q "Authorization" "$RUN_LOG"
tap_check "token unset: no Authorization header in any curl call" $?

# --- 3. Happy path ------------------------------------------------------------

reset_case
install_run happy
[ "$RUN_STATUS" -eq 0 ]
tap_check "happy path: installer exits 0" $?
[ -f "$RUN_HOME/.local/bin/relate" ]
tap_check "happy path: \$HOME/.local/bin/relate exists" $?
[ -x "$RUN_HOME/.local/bin/relate" ]
tap_check "happy path: installed file is executable" $?
cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-darwin-arm64"
tap_check "happy path: installed content matches the fixture" $?
[ "$(ls -A "$RUN_HOME/.local/bin")" = "relate" ]
tap_check "happy path: bin dir holds exactly the installed binary (no temp leftovers)" $?
grep -q '^-Y$' "$RUN_KEYGEN_LOG" && grep -q '^verify$' "$RUN_KEYGEN_LOG" &&
	grep -q '^relate-release$' "$RUN_KEYGEN_LOG"
tap_check "happy path: ssh-keygen -Y verify ran under the relate-release identity" $?
sig_line=$(grep -n "releases/assets/$ASSET_ID_SIG\$" "$RUN_LOG" | head -n 1 | cut -d: -f1)
bin_line=$(grep -n "releases/assets/$ASSET_ID_DARWIN\$" "$RUN_LOG" | head -n 1 | cut -d: -f1)
[ -n "$sig_line" ] && [ -n "$bin_line" ] && [ "$sig_line" -lt "$bin_line" ]
tap_check "happy path: SHA256SUMS.sig is downloaded before the binary asset" $?

# --- 3b. Signature verification ------------------------------------------------

reset_case
STUB_MODE=missing_sig
install_run sig-missing
[ "$RUN_STATUS" -ne 0 ]
tap_check "missing sig: installer dies non-zero" $?
grep -q "has no asset named SHA256SUMS.sig" "$RUN_ERR"
tap_check "missing sig: error names the missing SHA256SUMS.sig asset" $?
[ "$(asset_call_count)" -eq 0 ]
tap_check "missing sig: no /releases/assets/ download is attempted" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "missing sig: nothing installed" $?

reset_case
STUB_MODE=sig_invalid
install_run sig-invalid
[ "$RUN_STATUS" -ne 0 ]
tap_check "verify failure: installer dies non-zero" $?
grep -q "signature verification failed" "$RUN_ERR"
tap_check "verify failure: error mentions signature verification" $?
[ "$(asset_call_count)" -eq 2 ]
tap_check "verify failure: only SHA256SUMS and SHA256SUMS.sig were downloaded" $?
! grep -q "releases/assets/$ASSET_ID_DARWIN\$" "$RUN_LOG"
tap_check "verify failure: the binary asset is not downloaded before verification" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "verify failure: nothing at the final install path" $?

# --- 3b'. Signed version binding (ADR-0457) -----------------------------------

reset_case
STUB_MODE=sums_no_version
install_run sums-no-version
[ "$RUN_STATUS" -ne 0 ]
tap_check "no version header above the legacy ceiling: installer dies non-zero" $?
grep -q "has no signed version header" "$RUN_ERR"
tap_check "no version header above the legacy ceiling: error names the missing header" $?
[ "$(asset_call_count)" -eq 2 ]
tap_check "no version header above the legacy ceiling: only SHA256SUMS and SHA256SUMS.sig were downloaded" $?
! grep -q "releases/assets/$ASSET_ID_DARWIN\$" "$RUN_LOG"
tap_check "no version header above the legacy ceiling: the binary is not downloaded" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "no version header above the legacy ceiling: nothing installed" $?

reset_case
STUB_MODE=sums_wrong_version
install_run sums-wrong-version
[ "$RUN_STATUS" -ne 0 ]
tap_check "mismatched version header: installer dies non-zero" $?
grep -q "SHA256SUMS is for $SUMS_WRONG_TAG, not the requested release $TEST_TAG" "$RUN_ERR"
tap_check "mismatched version header: error names both versions" $?
[ "$(asset_call_count)" -eq 2 ]
tap_check "mismatched version header: only SHA256SUMS and SHA256SUMS.sig were downloaded" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "mismatched version header: nothing installed" $?

reset_case
STUB_MODE=sums_empty_version
install_run sums-empty-version
[ "$RUN_STATUS" -ne 0 ]
tap_check "empty version header: installer dies non-zero" $?
grep -q "carries an empty version header" "$RUN_ERR"
tap_check "empty version header: error names the empty header (not the legacy path)" $?
[ "$(asset_call_count)" -eq 2 ]
tap_check "empty version header: only SHA256SUMS and SHA256SUMS.sig were downloaded" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "empty version header: nothing installed" $?

reset_case
STUB_MODE=sums_no_space_header
install_run sums-no-space-header
[ "$RUN_STATUS" -ne 0 ]
tap_check "no-space header spelling: installer dies non-zero" $?
grep -q "has no signed version header" "$RUN_ERR"
tap_check "no-space header spelling: treated as headerless, like the Rust prefix" $?
[ "$(asset_call_count)" -eq 2 ]
tap_check "no-space header spelling: only SHA256SUMS and SHA256SUMS.sig were downloaded" $?

reset_case
STUB_MODE=sums_extra_space_header
install_run sums-extra-space-header
[ "$RUN_STATUS" -eq 0 ]
tap_check "extra-space header value: installer exits 0 (both parsers trim)" $?
cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-darwin-arm64"
tap_check "extra-space header value: installs the fixture binary" $?

reset_case
STUB_MODE=legacy_tag
install_run legacy-tag
[ "$RUN_STATUS" -eq 0 ]
tap_check "headerless manifest at/below the legacy ceiling: installer exits 0" $?
grep -q "legacy manifest" "$RUN_ERR"
tap_check "headerless manifest at/below the legacy ceiling: warns about the legacy manifest" $?
cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-darwin-arm64"
tap_check "headerless manifest at/below the legacy ceiling: installs the fixture binary" $?

reset_case
RUN_PATH="$nosshbin"
install_run no-ssh-keygen
[ "$RUN_STATUS" -ne 0 ]
tap_check "ssh-keygen absent: installer dies non-zero" $?
grep -q "openssh-client" "$RUN_ERR"
tap_check "ssh-keygen absent: error points at openssh-client" $?
[ "$(asset_call_count)" -eq 0 ]
tap_check "ssh-keygen absent: no asset download is attempted" $?

# --- 3c. Real ssh-keygen roundtrip ---------------------------------------------
# The keypair, signing and patched-installer machinery lives in the harness
# (build_real_roundtrip_fixtures); these cases only assert over its result.

build_real_roundtrip_fixtures

# Case 1: the patched copy differs from the shipped installer in exactly
# the SIGNING_KEY line. Each case spells its descriptions once (the d_
# variables) and skips under them when the probe failed.
d_patch="real ssh-keygen: patched copy differs only in the SIGNING_KEY line"
if [ -n "$real_roundtrip" ]; then
	[ "$(diff "$INSTALLER" "$patched_installer" | grep -c '^[<>]')" -eq 2 ]
	tap_check "$d_patch" $?
else
	skip_real_case "$d_patch"
fi

# Case 2: a real signature verifies and installs.
d_ok_status="real ssh-keygen: installer exits 0 with a real signature"
d_ok_content="real ssh-keygen: installed content matches the fixture"
d_ok_stub="real ssh-keygen: stub ssh-keygen was not invoked"
if [ -n "$real_roundtrip" ]; then
	real_install_run real-sig-ok "$real_fixtures"
	[ "$RUN_STATUS" -eq 0 ]
	tap_check "$d_ok_status" $?
	cmp -s "$RUN_HOME/.local/bin/relate" "$fixtures/relate-darwin-arm64"
	tap_check "$d_ok_content" $?
	[ ! -s "$RUN_KEYGEN_LOG" ]
	tap_check "$d_ok_stub" $?
else
	skip_real_case "$d_ok_status" "$d_ok_content" "$d_ok_stub"
fi

# Case 3: the second key's own fixture tree fails closed — verification
# against the first key must reject its differently-signed manifest.
d_wrong_status="real ssh-keygen, wrong signer key: installer dies non-zero"
d_wrong_err="real ssh-keygen, wrong signer key: error mentions signature verification"
d_wrong_installed="real ssh-keygen, wrong signer key: nothing installed"
if [ -n "$real_roundtrip" ]; then
	real_install_run real-sig-wrong-key "$wrong_fixtures"
	[ "$RUN_STATUS" -ne 0 ]
	tap_check "$d_wrong_status" $?
	grep -q "signature verification failed" "$RUN_ERR"
	tap_check "$d_wrong_err" $?
	[ ! -e "$RUN_HOME/.local/bin/relate" ]
	tap_check "$d_wrong_installed" $?
else
	skip_real_case "$d_wrong_status" "$d_wrong_err" "$d_wrong_installed"
fi

# --- 4. Checksum failure ------------------------------------------------------

reset_case
STUB_MODE=corrupt
install_run checksum-fail
[ "$RUN_STATUS" -ne 0 ]
tap_check "checksum fail: installer dies non-zero" $?
grep -q "checksum verification failed" "$RUN_ERR"
tap_check "checksum fail: error mentions checksum verification" $?
[ ! -e "$RUN_HOME/.local/bin/relate" ]
tap_check "checksum fail: nothing at the final install path" $?
[ -z "$(ls -A "$RUN_HOME/.local/bin" 2>/dev/null)" ]
tap_check "checksum fail: no temp names left in the bin dir" $?

# --- 5. Missing asset -----------------------------------------------------------

reset_case
STUB_MODE=missing_asset
install_run missing-asset
[ "$RUN_STATUS" -ne 0 ]
tap_check "missing asset: installer dies non-zero" $?
grep -q "has no asset named relate-darwin-arm64" "$RUN_ERR"
tap_check "missing asset: error names the missing asset" $?
[ "$(asset_call_count)" -eq 0 ]
tap_check "missing asset: no /releases/assets/ download is attempted" $?

# --- 6. Lookup failure ----------------------------------------------------------

reset_case
STUB_MODE=lookup_404
install_run lookup-fail
[ "$RUN_STATUS" -ne 0 ]
tap_check "lookup failure: installer dies non-zero" $?
grep -q "could not resolve latest release" "$RUN_ERR" &&
	grep -q "GITHUB_TOKEN" "$RUN_ERR"
tap_check "lookup failure: stderr carries the could-not-resolve message mentioning GITHUB_TOKEN" $?
[ "$(grep -c 'could not resolve latest release' "$INSTALLER")" -eq 1 ]
tap_check "lookup failure: the message text appears exactly once in install.sh" $?

# --- 7. Cleanliness -------------------------------------------------------------

[ -z "$(ls -A "$itmp")" ]
tap_check "cleanliness: installer temp dirs removed (TMPDIR empty after all runs)" $?

# --- summary --------------------------------------------------------------------

tap_done
