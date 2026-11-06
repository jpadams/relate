#!/usr/bin/env bash
# Compile and run the C examples in docs/api/c/examples against the FFI
# library named by scripts/ffi-lib.sh. Paths are relative to the repo
# root. Every .c file in the directory is an example — the list is the
# directory, never a second table; the OS branch (ffi-lib.sh's
# RELATE_FFI_LINK discriminator) selects only the compile command
# and the binary suffix (LABS-631).
set -euo pipefail

# shellcheck source=ffi-lib.sh
source scripts/ffi-lib.sh

if [[ -n "${RUNNER_TEMP:-}" ]]; then
  _example_dir="$RUNNER_TEMP"
else
  _example_dir="$(mktemp -d)"
fi

# Bound once, above the branch: the OS arms differ only in the compile
# command and the binary suffix.
_inc="-Isurfaces/relate-ffi/include"

if [[ "$RELATE_FFI_LINK" == msvc ]]; then
  # The MSVC linker wants the import library; the candidates are data,
  # first hit wins.
  _link_lib=""
  for _cand in "${RELATE_LIB}.lib" "$RELATE_LIB_DIR/relate_ffi.lib" "$RELATE_LIB_DIR/deps/relate_ffi.dll.lib"; do
    if [[ -f "$_cand" ]]; then
      _link_lib="$_cand"
      break
    fi
  done
  if [[ -z "$_link_lib" ]]; then
    printf 'ffi-compile-example: import library for %s not found\n' "$RELATE_LIB" >&2
    exit 1
  fi
  compile() { # $1 = source, $2 = binary
    cl -nologo "$_inc" "$1" -Fe:"$2" -link "$_link_lib"
  }
  _exe_suffix=.exe
else
  compile() { # $1 = source, $2 = binary
    cc "$_inc" "$1" -L "$RELATE_LIB_DIR" -lrelate_ffi -o "$2"
  }
  _exe_suffix=
fi

for _src in docs/api/c/examples/*.c; do
  _bin="$_example_dir/$(basename "$_src" .c)-doc-example$_exe_suffix"
  compile "$_src" "$_bin"
  "$_bin"
done
