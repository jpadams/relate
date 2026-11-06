# Behavioral tests for the extension release table
#
# Locks the release-table half of the ADR-0431 §1 channel contract:
# distribution/extension-packages.json is the one table the release
# workflow (release-extensions.yml) reads — each row names a package,
# the crate directory that builds its executable, and the crate's
# committed `extension.json`. The workflow validates a dispatch at
# run time (one row for the name, the paths exist, the manifest agrees,
# a strict X.Y.Z version, a non-empty executable); these tests mirror
# that validation repo-side so a broken row fails CI instead of a
# human's dispatch. The executable is the crate's cargo `[[bin]]` name
# and the tarball member (one convention, three uses), and every
# first-party child that ships a package manifest (an
# `extensions/*/package/extension.json`) carries a row — the importer
# extraction (ADR-0431 §11) is the channel's first content, so no
# child can silently miss the channel.
#
# Local run:
#   bats .github/scripts/tests/extension-packages-table.bats
#
# Requirements:
#   bats-core >= 1.5.0  (brew install bats-core)
#   bash, jq, awk

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$(dirname "$BATS_TEST_FILENAME")/../../.." && pwd)"
TABLE="${REPO_ROOT}/distribution/extension-packages.json"

# The crate's cargo binary name: the first `[[bin]]` block's `name`.
# One bin per first-party child is today's contract (ADR-0434); the
# awk reads the block's `name = "…"` line only inside it.
bin_name() {
  local crate_dir="$1"
  awk '/^\[\[bin\]\]/{in_bin=1; next}
       in_bin && /^name = /{sub(/^name = "/, ""); sub(/"$/, ""); print; exit}' \
    "${crate_dir}/Cargo.toml"
}

# ---------------------------------------------------------------------------
# the table
# ---------------------------------------------------------------------------

@test "the table parses with schema_version 1" {
  run jq -e '.schema_version == 1' "${TABLE}"
  [ "$status" -eq 0 ]
}

@test "each row carries name, crate_dir and manifest, exactly once per name" {
  # The shape the release workflow's resolve step reads, one row per
  # package: a missing field or a duplicated name is a dispatch-time
  # refusal — caught here first.
  run jq -e 'all(.packages[]; (keys | sort) == ["crate_dir", "manifest", "name"])' "${TABLE}"
  [ "$status" -eq 0 ]
  run jq -e '(.packages | map(.name) | length)
             == (.packages | map(.name) | unique | length)' "${TABLE}"
  [ "$status" -eq 0 ]
}

@test "every row's paths exist and the manifest names the row's package" {
  while read -r name crate_dir manifest; do
    [ -d "${REPO_ROOT}/${crate_dir}" ]
    [ -f "${REPO_ROOT}/${manifest}" ]
    [ "$(jq -r '.name' "${REPO_ROOT}/${manifest}")" = "${name}" ]
  done < <(jq -r '.packages[] | "\(.name) \(.crate_dir) \(.manifest)"' "${TABLE}")
}

@test "every manifest carries a strict X.Y.Z version and a non-empty executable" {
  while read -r manifest_path; do
    local manifest="${REPO_ROOT}/${manifest_path}"
    jq -e '.version | test("^[0-9]+\\.[0-9]+\\.[0-9]+$")' "${manifest}"
    jq -e '.executable != "" and .executable != null' "${manifest}"
  done < <(jq -r '.packages[].manifest' "${TABLE}")
}

@test "the executable is the crate's cargo bin name" {
  while read -r crate_dir manifest; do
    local repo_manifest="${REPO_ROOT}/${manifest}"
    [ "$(jq -r '.executable' "${repo_manifest}")" = "$(bin_name "${REPO_ROOT}/${crate_dir}")" ]
  done < <(jq -r '.packages[] | "\(.crate_dir) \(.manifest)"' "${TABLE}")
}

# ---------------------------------------------------------------------------
# completeness: no first-party child off the channel
# ---------------------------------------------------------------------------

@test "every shipped package manifest under extensions/ carries a row" {
  while read -r manifest; do
    local name
    name="$(jq -r '.name' "${manifest}")"
    [ "$(jq -r --arg name "${name}" \
          '[.packages[] | select(.name == $name)] | length' "${TABLE}")" = "1" ]
  done < <(find "${REPO_ROOT}/extensions" -path '*/package/extension.json' \
             | LC_ALL=C sort)
}
