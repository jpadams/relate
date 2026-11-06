#!/usr/bin/env bash
# Name the relate FFI library for this OS.
#
# Source (caller receives the exports; does not compile):
#   source scripts/ffi-lib.sh
# Sets RELATE_LIB to
# "${RELATE_LIB:-<repo root>/target/debug/<primary>}" (the repo root
# derived from this script's own location, like every sibling script)
# and prepends that library directory to DYLD_LIBRARY_PATH (Darwin), PATH
# (Windows), or LD_LIBRARY_PATH (other systems). A preset RELATE_LIB
# is left unchanged, and the loader path follows its directory. Also
# exports RELATE_FFI_LINK=msvc|cc — the link-style discriminator for
# the same uname case, so consumers branch on a typed value instead of
# re-sniffing the library filename — and RELATE_LIB_DIR, the library
# directory derived once here from RELATE_LIB, so consumers read it
# instead of re-spelling ${RELATE_LIB%/*} (LABS-631).
#
# Execute (prints basenames and does not export RELATE_LIB):
#   bash scripts/ffi-lib.sh --name
#   bash scripts/ffi-lib.sh --names

_ffi_sourced=0
if (return 0 2>/dev/null); then
  _ffi_sourced=1
fi

if [[ "$_ffi_sourced" -eq 0 ]]; then
  set -euo pipefail
fi

# One uname case names every library and the loader variable for this OS.
case "$(uname -s)" in
  Darwin)
    _ffi_primary=librelate_ffi.dylib
    _ffi_names=(librelate_ffi.dylib librelate_ffi.so)
    _ffi_loader=DYLD_LIBRARY_PATH
    _ffi_link=cc
    ;;
  MINGW* | MSYS* | CYGWIN*)
    _ffi_primary=relate_ffi.dll
    _ffi_names=(relate_ffi.dll)
    _ffi_loader=PATH
    _ffi_link=msvc
    ;;
  *)
    _ffi_primary=librelate_ffi.so
    _ffi_names=(librelate_ffi.so)
    _ffi_loader=LD_LIBRARY_PATH
    _ffi_link=cc
    ;;
esac

if [[ "$_ffi_sourced" -eq 0 ]]; then
  if [[ $# -ne 1 ]]; then
    printf 'ffi-lib: expected exactly one argument: --name or --names\n' >&2
    exit 1
  fi
  case "$1" in
    --name) printf '%s\n' "$_ffi_primary" ;;
    --names) printf '%s\n' "${_ffi_names[@]}" ;;
    *)
      printf 'ffi-lib: unknown argument: %s\n' "$1" >&2
      exit 1
      ;;
  esac
  exit 0
fi

# Resolve and validate before any export: RELATE_LIB_DIR is a
# cross-consumer contract, so a value whose directory component is
# unusable (absent or empty — a root-level path) fails loudly at this
# boundary, and the failure path leaves the caller's environment
# exactly as it found it.
_ffi_scratch="_ffi_sourced _ffi_primary _ffi_names _ffi_loader _ffi_link _ffi_lib _ffi_root _ffi_prev"
_ffi_cleanup() { # the scratch roster's single cleanup, both exits call it
  unset -f _ffi_cleanup
  unset $_ffi_scratch _ffi_scratch
}
_ffi_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_ffi_lib="${RELATE_LIB:-$_ffi_root/target/debug/$_ffi_primary}"
case "$_ffi_lib" in
  ?*/*) ;;
  *)
    printf 'ffi-lib: RELATE_LIB must name a path with a directory component: %s\n' "$_ffi_lib" >&2
    _ffi_cleanup
    return 1
    ;;
esac
export RELATE_LIB="$_ffi_lib"
export RELATE_FFI_LINK="$_ffi_link"
export RELATE_LIB_DIR="${_ffi_lib%/*}"
# The loader variable's name is data (the uname case above); the
# dynamic export honors it without a second branch over it.
_ffi_prev="${!_ffi_loader:-}"
export "$_ffi_loader=${RELATE_LIB_DIR}${_ffi_prev:+:$_ffi_prev}"

_ffi_cleanup
return 0
