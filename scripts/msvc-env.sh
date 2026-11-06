# MSVC linker environment for the Windows lanes (ADR-0251 §2). vcvars makes
# rustc and cl take link.exe from PATH, and Git Bash has GNU coreutils
# link.exe in /usr/bin ahead of the MSVC linker, so every step that invokes
# cargo or cl needs the MSVC bin directory at the front of PATH.
#
# SOURCE this script (`source scripts/msvc-env.sh …`): PATH does not persist
# between GitHub Actions steps, so each step that needs the linker sources
# it again. Options:
#
#   --require       fail the step if the MSVC linker directory is missing
#   --persist-env   also write CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER to
#                   $GITHUB_ENV so later steps inherit it
#   --label NAME    step label for the --require error message
#                   (default: ci)
#
# Requires ilammy/msvc-dev-cmd@v1 (or vcvars) to have run earlier in the
# job: VCToolsInstallDir and the VSCMD_ARG_*_ARCH variables come from it.

require=0
persist_env=0
label="ci"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --require) require=1 ;;
    --persist-env) persist_env=1 ;;
    --label) shift; label="$1" ;;
    *) printf 'msvc-env: unknown option: %s\n' "$1" >&2; return 2 2>/dev/null || exit 2 ;;
  esac
  shift
done

host="${VSCMD_ARG_HOST_ARCH:-x64}"
tgt="${VSCMD_ARG_TGT_ARCH:-x64}"
msvc_bin=$(cygpath -u "${VCToolsInstallDir}bin/Host${host^}/${tgt}")
if [[ "$require" == 1 && ! -d "$msvc_bin" ]]; then
  printf '%s: MSVC linker directory not found: %s\n' "$label" "$msvc_bin" >&2
  return 1 2>/dev/null || exit 1
fi
export PATH="${msvc_bin}:${PATH}"
linker=$(cygpath -w "$msvc_bin/link.exe")
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$linker"
if [[ "$persist_env" == 1 ]]; then
  echo "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$linker" >> "$GITHUB_ENV"
fi
