#!/usr/bin/env bash
# Shared build entry for the Swift surface's binary target (ADR-0433): builds relate-ffi as a static
# library per Apple slice and assembles target/apple/RelateFFI.xcframework.
#
# Slices:
#   all    ios-arm64, ios-arm64_x86_64-simulator, macos-arm64_x86_64
#   macos  macos-arm64_x86_64 only (enough for `swift test` on a Mac)
#
# --zip also writes target/apple/RelateFFI.xcframework.zip, the release
# asset SwiftPM downloads (ditto keeps the framework's symlinks and puts the
# .xcframework directory at the archive root, as SwiftPM requires).
#
# The C header is never copied into the repo: it is staged from
# surfaces/relate-ffi/include/relate.h into the XCFramework with a
# module map for the `CRelate` clang module. Headers sit in a
# `CRelate/` subdirectory so the module map cannot collide with another
# XCFramework's in an app's shared include path.
#
# Env:
#   IPHONEOS_DEPLOYMENT_TARGET  (default 16.0) — must match Package.swift
#   MACOSX_DEPLOYMENT_TARGET    (default 13.0) — must match Package.swift
#   XCFRAMEWORK_CARGO_PROFILE   (default release) — a profile name
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SLICES="all"
ZIP=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --slices) SLICES="$2"; shift 2 ;;
    --slices=*) SLICES="${1#*=}"; shift ;;
    --zip) ZIP=true; shift ;;
    -h|--help)
      echo "usage: $0 [--slices all|macos] [--zip]" >&2
      exit 0
      ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
PROFILE="${XCFRAMEWORK_CARGO_PROFILE:-release}"
OUT="$REPO/target/apple"
XCFW="$OUT/RelateFFI.xcframework"
LIB="librelate_ffi.a"

case "$SLICES" in
  all) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios) ;;
  macos) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  *) echo "unknown --slices value: $SLICES (want all|macos)" >&2; exit 2 ;;
esac

installed="$(rustup target list --installed)"
for t in "${TARGETS[@]}"; do
  grep -qx "$t" <<<"$installed" || {
    echo "missing Rust target $t: rustup target add ${TARGETS[*]}" >&2
    exit 1
  }
done

for t in "${TARGETS[@]}"; do
  cargo build -p relate-ffi --profile "$PROFILE" --target "$t" \
    --manifest-path "$REPO/Cargo.toml"
done

# cargo's `release` profile writes to target/<triple>/release; any other
# profile writes to target/<triple>/<profile>.
lib_for() { echo "$REPO/target/$1/$PROFILE/$LIB"; }

rm -rf "$OUT/headers" "$OUT/macos" "$OUT/ios-sim" "$XCFW"
mkdir -p "$OUT/headers/CRelate" "$OUT/macos" "$OUT/ios-sim"
cp "$REPO/surfaces/relate-ffi/include/relate.h" "$OUT/headers/CRelate/"
cat >"$OUT/headers/CRelate/module.modulemap" <<'EOF'
module CRelate {
    header "relate.h"
    link "iconv"
    link framework "CoreFoundation"
    export *
}
EOF

lipo -create "$(lib_for aarch64-apple-darwin)" "$(lib_for x86_64-apple-darwin)" \
  -output "$OUT/macos/$LIB"
args=(-library "$OUT/macos/$LIB" -headers "$OUT/headers")
if [[ "$SLICES" != macos ]]; then
  lipo -create "$(lib_for aarch64-apple-ios-sim)" "$(lib_for x86_64-apple-ios)" \
    -output "$OUT/ios-sim/$LIB"
  args+=(-library "$(lib_for aarch64-apple-ios)" -headers "$OUT/headers"
         -library "$OUT/ios-sim/$LIB" -headers "$OUT/headers")
fi

xcodebuild -create-xcframework "${args[@]}" -output "$XCFW"
echo "$XCFW"

if $ZIP; then
  rm -f "$XCFW.zip"
  ditto -c -k --keepParent "$XCFW" "$XCFW.zip"
  echo "$XCFW.zip"
fi
