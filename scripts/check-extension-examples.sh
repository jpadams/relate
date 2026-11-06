#!/usr/bin/env bash
#
# check-extension-examples.sh — check the shipped extension examples.
#
# docs/api/extensions/examples/ ships the hello-world children of the MCP
# stdio mount: bash (the kit plus the example that inlines it), Python (the
# official MCP SDK, mcp 2.x) and TypeScript (the SDK's Transport interface,
# @modelcontextprotocol/client 2.x). All three walk the live Stack Exchange
# API, so CI never executes them (network + API quota): this check keeps them
# honest at the parse level and, when the Python SDK is installed, exercises
# the example's own JSON-RPC parse path. The executed, offline coverage of the
# mount lives in the CLI's fixture suites.
#
# What runs, and what each step pins:
#   bash -n                     the kit and the example
#   the kit pin                 so-to-graph.sh's inlined block is the kit
#                               from mcp-client.sh, verbatim (the installer
#                               carries one executable, so the two copies
#                               must match)
#   python3 -m py_compile       the Python example
#   the Python parse path       when `python3 -c 'import mcp'` succeeds: the
#                               example module is imported and its envelope
#                               read and message parse are exercised through
#                               the SDK; without the SDK the step prints a
#                               note and passes (CI installs no SDK)
#   node --check                the TypeScript example, its type syntax
#                               stripped first by node's own
#                               stripTypeScriptTypes (needs Node >= 22.13)
#
# The bytecode cache and the stripped module go to a scratch directory, so
# the docs tree stays clean. Runs in CI's `clients` stage (the polyglot image
# carries node and python3); run it by hand from the repository root.
#
# Usage:
#   scripts/check-extension-examples.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLES="$REPO_ROOT/docs/api/extensions/examples"

fail() {
  echo "error: $*" >&2
  exit 2
}

for tool in bash python3 node; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is required (and was not found on PATH)"
done

echo "Extension example checks"
echo "  examples: $EXAMPLES"
echo

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

bash -n "$EXAMPLES/mcp-client.sh" || fail "mcp-client.sh does not parse"
echo "  ok    mcp-client.sh (bash -n)"

bash -n "$EXAMPLES/so-to-graph.sh" || fail "so-to-graph.sh does not parse"
echo "  ok    so-to-graph.sh (bash -n)"

# The inlined kit and the source kit are one text: a package ships one
# executable, so the example carries the kit verbatim. mcp-client.sh is its
# header (shebang + the sourcing note) and then the kit; the kit starts at
# its own first line.
sed -n '/^# >>> mcp-client.sh (verbatim/,/^# <<< mcp-client.sh <<</p' "$EXAMPLES/so-to-graph.sh" |
  sed '1d;$d' >"$scratch/inlined-kit.sh"
sed -n '/^# The MCP revision this kit speaks/,$p' "$EXAMPLES/mcp-client.sh" >"$scratch/kit.sh"
if ! cmp -s "$scratch/inlined-kit.sh" "$scratch/kit.sh"; then
  diff -u "$scratch/kit.sh" "$scratch/inlined-kit.sh" >&2 || true
  fail "so-to-graph.sh's inlined kit differs from mcp-client.sh; re-inline it between the markers"
fi
echo "  ok    so-to-graph.sh inlines mcp-client.sh verbatim"

PYTHONPYCACHEPREFIX="$scratch/pycache" python3 -m py_compile "$EXAMPLES/so-to-graph.py" ||
  fail "so-to-graph.py does not compile"
echo "  ok    so-to-graph.py (python3 -m py_compile)"

cat >"$scratch/check_python_example.py" <<'CHECK_PYTHON'
import importlib.util
import io
import json
import pathlib
import sys

examples = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("so_to_graph", examples / "so-to-graph.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

# The transport's first act, on a stream that carries one envelope line.
envelope = module.read_envelope(
    io.StringIO(json.dumps({"tag": "rust", "pages": 1, "body": True}) + "\n")
)
assert envelope["tag"] == "rust", envelope

# The SDK's own parse path, on a real JSON-RPC line.
message = module.parse_message('{"jsonrpc":"2.0","id":1,"method":"tools/list"}')
assert message.message.method == "tools/list", message
CHECK_PYTHON

if python3 -c 'import mcp' >/dev/null 2>&1; then
  PYTHONPYCACHEPREFIX="$scratch/pycache" python3 "$scratch/check_python_example.py" "$EXAMPLES" ||
    fail "so-to-graph.py's envelope read or message parse failed"
  echo "  ok    so-to-graph.py (imports and parses through the SDK)"
else
  echo "  skip  so-to-graph.py parse check (the mcp SDK is not installed; pip install 'mcp>=2')"
fi

# `node --check` reads JavaScript only, so the type syntax comes off first
# through node's own stripper; the result is a module and is checked as one.
node --no-warnings -e '
const { stripTypeScriptTypes } = require("node:module");
const fs = require("node:fs");
if (typeof stripTypeScriptTypes !== "function") {
  console.error("node " + process.version + " cannot strip type syntax; need >= 22.13");
  process.exit(2);
}
process.stdout.write(stripTypeScriptTypes(fs.readFileSync(process.argv[1], "utf8")));
' "$EXAMPLES/so-to-graph.ts" >"$scratch/so-to-graph.mjs" ||
  fail "so-to-graph.ts: the type strip failed"
node --check "$scratch/so-to-graph.mjs" || fail "so-to-graph.ts does not parse"
echo "  ok    so-to-graph.ts (node --check, types stripped)"

echo
echo "All shipped extension examples check out."
