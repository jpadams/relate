#!/usr/bin/env bash
# Shared TOML readers for repo scripts. Source this file from a script that
# has resolved the repo root, e.g.:
#   REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
#   source "$REPO/scripts/lib/toml.sh"
# Do not paste the awk programs into callers.

# Print the `version = "..."` value inside one exact TOML section header.
# Usage: toml_section_version <file> <section-header, e.g. [workspace.package]>
toml_section_version() {
  awk -v section="$2" '
    /^\[/ { in_sec = ($0 == section) }
    in_sec && /^version = "/ {
      v = $0; sub(/^version = "/, "", v); sub(/".*$/, "", v)
      print v; exit
    }
  ' "$1"
}

# Print the version of one package entry in a Cargo.lock file. Returns
# non-zero when the package is absent — failure is a real exit status, not
# an empty string callers must sniff for.
# Usage: toml_lock_package_version <Cargo.lock> <package-name>
toml_lock_package_version() {
  awk -v pkg="$2" '
    $0 == "name = \"" pkg "\"" { in_pkg = 1; next }
    in_pkg && /^version = / { gsub(/"/, "", $3); print $3; found = 1; exit }
    /^\[\[package\]\]/ { in_pkg = 0 }
    END { if (!found) exit 1 }
  ' "$1"
}
