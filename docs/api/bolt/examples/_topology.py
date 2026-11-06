"""The one topology policy for the Bolt doc examples.

The topology's one source is scripts/bolt-smoke.sh, which exports
RELATE_BOLT_PORT_IDENTITY (the `--bolt-identity` server the current
python driver needs — it rejects the honest `Relate/` agent) and
RELATE_BOLT_HOST beside the spawn. These examples run in CI through
that script; a manual run exports the variables beside its own server.
Every fact is required: unset or blank is a configuration error, never
a silent default.

This module is a deliberate parallel of
surfaces/relate-bolt/tests/_topology.py: this tree must stay
self-contained (the .md instructions run these scripts without the
tests tree on sys.path), and the tests tree does not reach into docs
for code — so each carries the same policy text, kept identical on
purpose.
"""
import os

_TOPOLOGY_SOURCE = (
    "must be set — run via scripts/bolt-smoke.sh or export the topology "
    "beside your own servers"
)


def required_env(name):
    """Return the named topology fact, failing loudly when absent."""
    value = os.environ.get(name)
    if value is None or not value.strip():
        raise RuntimeError(f"{name} {_TOPOLOGY_SOURCE}")
    return value


def bolt_uri(name):
    """The whole connection target from the one source: scheme, host,
    port — nothing hand-spliced at the call site."""
    return f"bolt://{required_env('RELATE_BOLT_HOST')}:{required_env(name)}"
