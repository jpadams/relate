"""The one topology policy for the official-python test tree.

The topology's one source is scripts/bolt-smoke.sh, which exports every
fact beside its spawn: RELATE_BOLT_HOST, the RELATE_BOLT_PORT_*
names, and RELATE_BOLT_IDENTITY (the `--bolt-identity` shim the
current python driver needs — it rejects the honest `Relate/`
agent). Every fact is required: unset or blank is a configuration
error, never a silent default and never a skip.

This module is a deliberate parallel of
docs/api/bolt/examples/_topology.py: the examples tree must stay
self-contained (its .md instructions run those scripts without the
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


def routing_uri(name):
    """The routing-scheme target (neo4j://) over the same facts — the
    compat suite's ROUTE probe dials this spelling."""
    return f"neo4j://{required_env('RELATE_BOLT_HOST')}:{required_env(name)}"
