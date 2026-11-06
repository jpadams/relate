# Harness machinery for the install.sh test harness
# (scripts/tests/test_install_sh.sh): the fixture and stub factories, the
# per-case run harness, the real-ssh-keygen roundtrip machinery, and the
# argv-log helpers.
#
# Sourced after `tap_sandbox` has set `$sandbox`, after tap.sh (for the
# `tap_skip` the roundtrip skips use) and after scripts/lib/common.sh (for
# `sha256_of`); the caller then runs `install_sh_fixtures`,
# `install_sh_stubs` and `build_real_roundtrip_fixtures`. Never executed
# directly.
#
# The two factories build the release-asset fixtures and the PATH stubs the
# installer is exercised against; their shared table is below.

# The shared table: the asset-id vocabulary (release.json generation, every
# assertion, and the stub curl's URL matching via the STUB_ASSET_* env
# install_run forwards) and the fixture tag/prefix spellings the fixture
# writers and assertions consult.
ASSET_ID_DARWIN=575443262
ASSET_ID_LINUX=575443263
ASSET_ID_SUMS=575443264
ASSET_ID_SIG=575443265

# The tag the default fixture release resolves to. The signed SHA256SUMS
# fixture names it in its ADR-0457 version header, and the release JSON
# reports it.
TEST_TAG=v9.9.9

# A tag the default release must not match: the wrong-version fixture names
# it in the header, and the assertion expects the mismatch.
SUMS_WRONG_TAG=v9.9.8

# The ADR-0457 header prefix as the fixture writer spells it. Section 0 of
# the harness holds this and install.sh's copy to manifest.rs's constant.
SUMS_VERSION_PREFIX='# relate-release-version: '

# One GitHub asset object, in GitHub's serialization order
# (url -> id -> node_id -> name), which install.sh's awk helper relies on.
# Every field derives from the table, including the release tag in the
# browser URL.
asset_obj() { # $1 = id, $2 = name, $3 = tag (default TEST_TAG)
	tag="${3:-$TEST_TAG}"
	cat <<EOF
    {
      "url": "https://api.github.com/repos/relatedb/relate/releases/assets/$1",
      "id": $1,
      "node_id": "RA_kwDOAAAAAc4iVKT$1",
      "name": "$2",
      "label": "",
      "content_type": "application/octet-stream",
      "state": "uploaded",
      "size": 42,
      "download_count": 0,
      "created_at": "2026-09-01T00:00:00Z",
      "updated_at": "2026-09-01T00:00:00Z",
      "browser_download_url": "https://github.com/relatedb/relate/releases/download/$tag/$2"
    }
EOF
}

release_json_head() { # $1 = tag
	tag="$1"
	cat <<EOF
{
  "url": "https://api.github.com/repos/relatedb/relate/releases/231946846",
  "tag_name": "$tag",
  "name": "$tag",
  "draft": false,
  "prerelease": false,
  "assets": [
EOF
}

# Write a release JSON for tag $1 to path $2, listing the `id:name` asset
# pairs $3.. (e.g. `575443262:relate-darwin-arm64`), comma-joined.
write_release_fixture() { # $1 = tag, $2 = output path, $3.. = id:name pairs
	_tag="$1"
	_out="$2"
	shift 2
	{
		release_json_head "$_tag"
		_first=1
		for _pair in "$@"; do
			[ "$_first" -eq 1 ] || printf ',\n'
			_first=0
			asset_obj "${_pair%%:*}" "${_pair#*:}" "$_tag"
		done
		printf '\n  ]\n}\n'
	} >"$_out"
}

# Build the release-asset fixtures under $fixtures: the two platform
# binaries, their corrupted variant, SHA256SUMS (with the ADR-0457 header)
# and its header variants, the signature placeholder, and the release JSON
# documents the stub serves. The caller sets LEGACY_MANIFEST_TAG (the guarded
# ceiling, `vX.Y.Z`) first.
install_sh_fixtures() {
	cat >"$fixtures/relate-darwin-arm64" <<'EOF'
#!/bin/sh
echo "relate fixture darwin-arm64"
EOF

	cat >"$fixtures/relate-linux-amd64" <<'EOF'
#!/bin/sh
echo "relate fixture linux-amd64"
EOF

	# Corrupted variant of the darwin binary for the checksum-fail case.
	{ cat "$fixtures/relate-darwin-arm64"; printf 'corrupt-bytes'; } \
		>"$fixtures/relate-darwin-arm64.corrupt"

	# SHA256SUMS in the format install.sh consumes: the ADR-0457 version
	# header first, then "<hex>  <name>" (two spaces).
	{
		printf '%s%s\n' "$SUMS_VERSION_PREFIX" "$TEST_TAG"
		printf '%s  %s\n' "$(sha256_of "$fixtures/relate-darwin-arm64")" relate-darwin-arm64
		printf '%s  %s\n' "$(sha256_of "$fixtures/relate-linux-amd64")" relate-linux-amd64
	} >"$fixtures/SHA256SUMS"

	# Headerless variant: a pre-ADR-0457 legacy manifest.
	sed '1d' "$fixtures/SHA256SUMS" >"$fixtures/SHA256SUMS.no-version"

	# Header naming another release: a replay/mismatch the installer must
	# reject.
	sed "1s|.*|${SUMS_VERSION_PREFIX}${SUMS_WRONG_TAG}|" \
		"$fixtures/SHA256SUMS" >"$fixtures/SHA256SUMS.wrong-version"

	# Present-but-empty header: a publish bug, not a legacy manifest — it
	# must fail hard, mirroring the Rust empty-header arm.
	sed "1s|.*|${SUMS_VERSION_PREFIX}|" \
		"$fixtures/SHA256SUMS" >"$fixtures/SHA256SUMS.empty-version"

	# No-space spelling (the prefix's trailing space removed): the Rust
	# parser's prefix literal requires it, so this is a headerless legacy
	# manifest.
	sed "1s|.*|${SUMS_VERSION_PREFIX% }${TEST_TAG}|" \
		"$fixtures/SHA256SUMS" >"$fixtures/SHA256SUMS.no-space-header"

	# Extra leading whitespace in the value: Rust trims both ends before
	# comparing, so the installer must too (the written form has exactly one
	# space, but the parsers agree on the tolerance).
	sed "1s|.*|${SUMS_VERSION_PREFIX} ${TEST_TAG}|" \
		"$fixtures/SHA256SUMS" >"$fixtures/SHA256SUMS.extra-space-header"

	# The signature is opaque to the installer (it only feeds the file to
	# ssh-keygen), so the fixture just needs to exist as a distinct asset.
	printf 'untrusted comment: relate test signature fixture\n' \
		>"$fixtures/SHA256SUMS.sig"

	# The four documents the stub's release-latest arm serves.
	write_release_fixture "$TEST_TAG" "$fixtures/release.json" \
		"$ASSET_ID_DARWIN:relate-darwin-arm64" \
		"$ASSET_ID_LINUX:relate-linux-amd64" \
		"$ASSET_ID_SUMS:SHA256SUMS" \
		"$ASSET_ID_SIG:SHA256SUMS.sig"
	# Without the darwin-arm64 asset, for the missing-asset case.
	write_release_fixture "$TEST_TAG" "$fixtures/release-missing.json" \
		"$ASSET_ID_LINUX:relate-linux-amd64" \
		"$ASSET_ID_SUMS:SHA256SUMS" \
		"$ASSET_ID_SIG:SHA256SUMS.sig"
	# Without the SHA256SUMS.sig asset, for the missing-signature case.
	write_release_fixture "$TEST_TAG" "$fixtures/release-missing-sig.json" \
		"$ASSET_ID_DARWIN:relate-darwin-arm64" \
		"$ASSET_ID_SUMS:SHA256SUMS"
	# The last pre-header release (ADR-0457 §3): its headerless manifest is
	# the legacy window the installer must still accept. The tag is the
	# guarded ceiling the caller extracted from manifest.rs, so the fixture
	# cannot drift from the constants the guards compare.
	write_release_fixture "$LEGACY_MANIFEST_TAG" "$fixtures/release-legacy-tag.json" \
		"$ASSET_ID_DARWIN:relate-darwin-arm64" \
		"$ASSET_ID_LINUX:relate-linux-amd64" \
		"$ASSET_ID_SUMS:SHA256SUMS" \
		"$ASSET_ID_SIG:SHA256SUMS.sig"
}

# The STUB_MODE vocabulary: every mode a case may set. install_run checks
# the per-case STUB_MODE against this list before launching the installer,
# so a typo or a half-renamed mode fails loudly instead of silently serving
# the happy-path fixtures. `ok` is the neutral value reset_case restores.
STUB_MODES='ok lookup_404 missing_asset missing_sig legacy_tag corrupt sig_invalid'
STUB_MODES="$STUB_MODES sums_no_version sums_wrong_version sums_empty_version"
STUB_MODES="$STUB_MODES sums_no_space_header sums_extra_space_header"

# Build the PATH stubs under $stubbin and sets `nosshbin` (the same stubs
# minus ssh-keygen, for the openssh-client-absent case). The stub curl never
# touches the network: it logs its argv (one arg per line, calls separated by
# an =CALL= marker) and serves fixtures keyed on the request URL, with
# failure modes selected via STUB_MODE.
install_sh_stubs() {
	# The stub's URL matching reads the asset ids from the STUB_ASSET_* env,
	# so the table above stays the only spelling.
	cat >"$stubbin/curl" <<'EOF'
#!/bin/sh
# Stub curl: logs argv (one arg per line, calls separated by =CALL=) and
# serves fixtures based on the URL (last argument). Never uses the network.
{
	printf '=CALL=\n'
	for arg in "$@"; do printf '%s\n' "$arg"; done
} >>"$STUB_LOG"

url=""
for arg in "$@"; do url="$arg"; done

case "$STUB_MODE" in
lookup_404) exit 22 ;;
esac

case "$url" in
*/releases/latest)
	case "$STUB_MODE" in
	missing_asset) cat "$STUB_FIXTURES/release-missing.json" ;;
	missing_sig) cat "$STUB_FIXTURES/release-missing-sig.json" ;;
	legacy_tag) cat "$STUB_FIXTURES/release-legacy-tag.json" ;;
	*) cat "$STUB_FIXTURES/release.json" ;;
	esac
	;;
*/releases/assets/$STUB_ASSET_DARWIN)
	case "$STUB_MODE" in
	corrupt) cat "$STUB_FIXTURES/relate-darwin-arm64.corrupt" ;;
	*) cat "$STUB_FIXTURES/relate-darwin-arm64" ;;
	esac
	;;
*/releases/assets/$STUB_ASSET_LINUX)
	cat "$STUB_FIXTURES/relate-linux-amd64"
	;;
*/releases/assets/$STUB_ASSET_SUMS)
	case "$STUB_MODE" in
	sums_no_version | legacy_tag) cat "$STUB_FIXTURES/SHA256SUMS.no-version" ;;
	sums_wrong_version) cat "$STUB_FIXTURES/SHA256SUMS.wrong-version" ;;
	sums_empty_version) cat "$STUB_FIXTURES/SHA256SUMS.empty-version" ;;
	sums_no_space_header) cat "$STUB_FIXTURES/SHA256SUMS.no-space-header" ;;
	sums_extra_space_header) cat "$STUB_FIXTURES/SHA256SUMS.extra-space-header" ;;
	*) cat "$STUB_FIXTURES/SHA256SUMS" ;;
	esac
	;;
*/releases/assets/$STUB_ASSET_SIG)
	cat "$STUB_FIXTURES/SHA256SUMS.sig"
	;;
*)
	echo "stub curl: unexpected URL: $url" >&2
	exit 22
	;;
esac
EOF

	cat >"$stubbin/uname" <<'EOF'
#!/bin/sh
# Stub uname: reports the OS/arch under test.
case "${1:-}" in
-s) printf '%s\n' "$STUB_UNAME_S" ;;
-m) printf '%s\n' "$STUB_UNAME_M" ;;
*) printf '%s\n' "$STUB_UNAME_S" ;;
esac
EOF

	chmod +x "$stubbin/curl" "$stubbin/uname"

	cat >"$stubbin/ssh-keygen" <<'EOF'
#!/bin/sh
# Stub ssh-keygen: logs argv in the same =CALL= format as stub curl, then
# succeeds or fails based on the mode env var. It never inspects the signature
# or stdin; the harness only exercises install.sh's control flow.
{
	printf '=CALL=\n'
	for arg in "$@"; do printf '%s\n' "$arg"; done
} >>"$STUB_KEYGEN_LOG"

case "$STUB_MODE" in
sig_invalid) exit 1 ;;
*) exit 0 ;;
esac
EOF

	chmod +x "$stubbin/ssh-keygen"

	# Same stubs minus ssh-keygen, for the "openssh-client absent" case. The
	# sandbox cannot rely on deleting it from $stubbin (other cases need it),
	# and /usr/bin cannot be left on PATH because the host may ship a real
	# ssh-keygen.
	nosshbin="$sandbox/stub-bin-no-ssh"
	mkdir -p "$nosshbin"
	cp "$stubbin/curl" "$stubbin/uname" "$nosshbin/"
	chmod +x "$nosshbin/curl" "$nosshbin/uname"
}

# Reset the per-case knobs to their defaults. The case sets STUB_MODE,
# STUB_UNAME_S/_M, GITHUB_TOKEN, RUN_PATH or RUN_INSTALLER after calling it.
reset_case() {
	STUB_MODE=ok
	STUB_UNAME_S=Darwin
	STUB_UNAME_M=arm64
	GITHUB_TOKEN=
	STUB_FIXTURES_DIR=$fixtures
	RUN_PATH="$stubbin:/usr/bin:/bin"
	RUN_INSTALLER=$INSTALLER
}

# Run the installer fully sandboxed. Populates RUN_STATUS, RUN_HOME, RUN_LOG
# (stub curl argv log) and RUN_ERR (installer stderr). RUN_INSTALLER selects
# the script under test: the shipped install.sh everywhere except the
# real-roundtrip cases, which run a sed-patched copy (see section 3c).
install_run() { # $1 = case name
	case_name=$1
	stub_mode=${STUB_MODE:-<unset>}
	case " $STUB_MODES " in
	*" $stub_mode "*) ;;
	*)
		echo "install_run: case $case_name: unexpected STUB_MODE: $stub_mode" >&2
		exit 2
		;;
	esac
	RUN_HOME="$sandbox/home-$case_name"
	RUN_LOG="$sandbox/curl-$case_name.log"
	RUN_KEYGEN_LOG="$sandbox/keygen-$case_name.log"
	RUN_ERR="$sandbox/err-$case_name.log"
	mkdir -p "$RUN_HOME"
	: >"$RUN_LOG"
	: >"$RUN_KEYGEN_LOG"
	env -i \
		HOME="$RUN_HOME" \
		PATH="$RUN_PATH" \
		TMPDIR="$itmp" \
		STUB_LOG="$RUN_LOG" \
		STUB_KEYGEN_LOG="$RUN_KEYGEN_LOG" \
		STUB_FIXTURES="$STUB_FIXTURES_DIR" \
		STUB_MODE="$STUB_MODE" \
		STUB_UNAME_S="$STUB_UNAME_S" \
		STUB_UNAME_M="$STUB_UNAME_M" \
		STUB_ASSET_DARWIN="$ASSET_ID_DARWIN" \
		STUB_ASSET_LINUX="$ASSET_ID_LINUX" \
		STUB_ASSET_SUMS="$ASSET_ID_SUMS" \
		STUB_ASSET_SIG="$ASSET_ID_SIG" \
		${GITHUB_TOKEN:+GITHUB_TOKEN="$GITHUB_TOKEN"} \
		/bin/sh "$RUN_INSTALLER" >"$sandbox/out-$case_name.log" 2>"$RUN_ERR"
	RUN_STATUS=$?
}

# Log helpers over the stub curl argv log.

# Every logged call carried the given argument (exact line match).
every_call_has() {
	awk -v want="$1" '
		/^=CALL=$/ { if (n > 0 && !found) bad++; n++; found=0; next }
		$0 == want { found=1 }
		END { if (n > 0 && !found) bad++; exit (n == 0 || bad > 0) }
	' "$RUN_LOG"
}

# Every logged call to a /releases/assets/ URL carried the given argument.
asset_calls_have() {
	awk -v want="$1" '
		/^=CALL=$/ { if (isasset && !found) bad++; isasset=0; found=0; next }
		/\/releases\/assets\// { isasset=1 }
		$0 == want { found=1 }
		END { if (isasset && !found) bad++; exit (bad > 0) }
	' "$RUN_LOG"
}

asset_call_count() {
	grep -c '/releases/assets/' "$RUN_LOG"
}

# --- real ssh-keygen roundtrip machinery -------------------------------------
# These cases exercise the real verify path end to end: the stub curl still
# serves fixtures offline, but the stub ssh-keygen is removed from PATH so
# install.sh invokes the real ssh-keygen against a throwaway Ed25519 keypair
# generated inside the sandbox (it never leaves it). The shipped script pins
# SIGNING_KEY as a constant, so these cases run a sed-patched COPY of
# install.sh (built inside the sandbox, never touching the repo file) whose
# pinned-key line is replaced with the throwaway public key; every other
# case runs the shipped script byte-for-byte.
real_roundtrip=""
real_skip_reason=""

# Generates a throwaway signing keypair at $1.
make_key() {
	"$real_keygen" -t ed25519 -N '' -C relate-release \
		-f "$1" </dev/null >/dev/null 2>&1
}

# Signs the SHA256SUMS in fixture tree $2 with key $1. The rm is
# load-bearing: ssh-keygen -Y sign refuses to overwrite an existing .sig
# without a tty, so the copied placeholder signature must go first.
sign_manifest() {
	rm -f "$2/SHA256SUMS.sig"
	"$real_keygen" -Y sign -f "$1" -n relate-release \
		"$2/SHA256SUMS" </dev/null >/dev/null 2>&1
}

# The real-roundtrip install protocol: run the patched installer copy
# against fixture tree $2 under the stub-curl case name $1. Call only
# under real_roundtrip=1 — patched_installer and keygen_dir exist only
# after build_real_roundtrip_fixtures.
real_install_run() {
	reset_case
	STUB_FIXTURES_DIR=$2
	RUN_INSTALLER=$patched_installer
	RUN_PATH="$nosshbin:$keygen_dir:/usr/bin:/bin"
	install_run "$1"
}

# The skip arm every case shares: one tap_skip per description, under
# the failure that actually happened.
skip_real_case() {
	for d in "$@"; do
		tap_skip "$d" "$real_skip_reason"
	done
}

# The one availability probe and fixture build for the three
# real-roundtrip cases — the patched installer copy, the first key's
# signed tree (cases 1-2), and the second key's own tree (case 3) — as
# flat early-return guards, each failure reason beside its own command:
# an absent binary and a present-but-failing keygen are different skips.
# Sets real_roundtrip=1 only when the cases can consume ready fixtures
# untouched.
build_real_roundtrip_fixtures() {
	real_keygen=$(command -v ssh-keygen 2>/dev/null)
	real_fixtures="$sandbox/fixtures-real"
	[ -n "$real_keygen" ] || {
		real_skip_reason="ssh-keygen not available"
		return 1
	}
	make_key "$sandbox/realkey" || {
		real_skip_reason="ssh-keygen present but the first throwaway keypair failed"
		return 1
	}
	throwaway_pub=$(cat "$sandbox/realkey.pub")
	keygen_dir=$(dirname "$real_keygen")
	# Patched copy of the shipped installer: only the SIGNING_KEY='...'
	# line is substituted, pointing verification at the throwaway key.
	patched_installer="$sandbox/install-patched.sh"
	sed "s|^SIGNING_KEY='.*'\$|SIGNING_KEY='$throwaway_pub'|" \
		"$INSTALLER" >"$patched_installer" || {
		real_skip_reason="building the patched installer copy failed"
		return 1
	}
	# The first key's tree: the stub's fixture tree with a real
	# signature over SHA256SUMS made with the throwaway key.
	mkdir -p "$real_fixtures" &&
		cp "$fixtures"/* "$real_fixtures"/ || {
		real_skip_reason="building the first key's fixture tree failed"
		return 1
	}
	sign_manifest "$sandbox/realkey" "$real_fixtures" || {
		real_skip_reason="ssh-keygen present but signing the fixture manifest failed"
		return 1
	}
	# Case 3's own tree: a byte-identical manifest copy carrying the
	# SECOND key's signature — the cases never mutate a shared fixture.
	wrong_fixtures="$sandbox/fixtures-real-wrong"
	cp -r "$real_fixtures" "$wrong_fixtures" || {
		real_skip_reason="building the wrong-key fixture tree failed"
		return 1
	}
	make_key "$sandbox/realkey2" || {
		real_skip_reason="ssh-keygen present but the second throwaway keypair failed"
		return 1
	}
	sign_manifest "$sandbox/realkey2" "$wrong_fixtures" || {
		real_skip_reason="ssh-keygen present but the wrong-key signature failed"
		return 1
	}
	real_roundtrip=1
}
