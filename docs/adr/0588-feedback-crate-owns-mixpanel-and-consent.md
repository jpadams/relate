# ADR-0588: One feedback crate owns the Mixpanel client and consent

Status: Proposed
Claim: 47fe8c879275aa42286f25d3d322ab7215b4d08e

## Context

The CLI already reports anonymous usage events to Mixpanel from
`surfaces/relate-cli/src/telemetry` (~2.4k lines): the event taxonomy, the
layered opt-out, a spool with a drain thread, and a hand-rolled `/track`
POST (ADR-0079, ADR-0263, ADR-0545). A second sender is joining it:
person-consented agent feedback (`submit`/`enable`/`disable`/`status` plus a
`feedback_submit` MCP tool) with its own report model, local validation and
redaction, a consent record, local receipts, and one POST per report.

Both send JSON to a Mixpanel `/track` endpoint under the same rules: an
endpoint allow-list, a short deadline, no retries, redirects never followed,
`ip=0`, and dedup by insert id. Two copies would drift apart and grow the
CLI binary crate instead of giving the feature a home. ADR-0079 rejected a
separate telemetry crate because the CLI was its only consumer, to be
promoted when a second consumer arrived; it is here, and ADR-0103's layout
lets a `surfaces/*` crate depend on `relate/*` and other surfaces while the
CLI depends on it, never the reverse.

## Decision

1. **One crate.** `surfaces/relate-feedback` owns:
   - the Mixpanel client: the `/track` POST (batched bodies, `ip=0`, a short
     deadline, no retries, redirects never followed, `$insert_id` dedup),
     response classification, and the one endpoint-allow-list
     implementation — `https` for the hosts a caller's policy names, and
     `http` only for loopback (ADR-0263's rule as the floor);
   - the consent and opt-out model: the layered on/off resolution
     (`DO_NOT_TRACK`, environment, config key), the one-time notice claim,
     the telemetry state file, and the feedback consent record (endpoint,
     authorized-until);
   - the feedback domain: report model and limits, the validation and
     redaction policy, receipts, submit orchestration, and the
     `feedback_submit` tool with its MCP message loop.

   The crate takes the config directory and an environment lookup as
   arguments; the CLI keeps the one config-directory rule.
2. **What telemetry keeps.** Out of `surfaces/relate-cli/src/telemetry`
   move the POST path and endpoint resolution, the opt-out resolution, and
   the state read/write (which includes the install id, so one file keeps
   one owner). Staying: the event taxonomy with its compile-time privacy
   contract, the envelope builder, the spool and drain-thread lifecycle,
   the telemetry verbs, and the e2e suite. No event, property, or opt-out
   semantic changes: the consolidation is plumbing, and `e2e_telemetry`
   passes unchanged.
3. **Two Mixpanel projects, one client.** Telemetry keeps its compiled-in
   usage-metrics token and `RELATE-CLI_` event prefix, with the existing
   `RELATE_MIXPANEL_TOKEN`/`RELATE_MIXPANEL_ENDPOINT` overrides. Feedback
   ships its own compiled-in project token and reports
   `RELATE-CLI_AGENT_FEEDBACK`, so free-text reports live in a project
   separate from anonymous usage counts, with `RELATE_FEEDBACK_TOKEN` and
   `RELATE_FEEDBACK_ENDPOINT` overrides. Ingestion tokens are client-side
   and public in every shipped binary by design (ADR-0079 §7); nothing
   injects a token at build time, and a build whose token is empty has no
   default destination and can send only to a loopback endpoint. The
   endpoint is resolved when the person authorizes (`feedback enable`);
   the token itself is read at send time. Each
   surface passes its endpoint policy as data: telemetry accepts any
   `https` host, feedback only the Mixpanel hosts. The feedback destination
   is recorded when the person authorizes (`feedback enable`); a later
   environment change cannot divert it, because submit and serve send to
   the recorded endpoint, never to whatever `RELATE_FEEDBACK_ENDPOINT`
   names at the time. Provisioning both projects is an operations task.
4. **The serve path is a hidden CLI command.** `relate feedback serve` is a
   hidden positional verb in the `feedback` family that speaks
   newline-delimited JSON-RPC 2.0 on its own stdin/stdout — hand-rolled over
   `serde_json` with no MCP SDK (ADR-0232's charter) — and exposes exactly
   one tool, `feedback_submit`. The crate owns the message loop and tool
   table, generic over reader and writer; the command wires process stdio.
   It is not an extension-program child.
5. **Bundles.** A new `feedback` Cargo feature carries the verbs, the serve
   path, and the crate; it is in `full` (the product default) and `agent`
   (agents are the audience), and absent from `minimal`. The `telemetry`
   feature gains `dep:relate-feedback` because its POST path now lives
   there. Feedback state stays under `feedback.*` keys in the CLI's config
   file.

## Alternatives considered

- **Keep the client in the CLI crate and add a second one for feedback** —
  rejected: two endpoint policies, deadlines, redirect rules and dedup
  paths to keep in step, and the CLI crate grows instead of the feature
  gaining a home.
- **A `relate-feedback-client` crate under `relate/`** — rejected: it is not
  engine code and no engine crate needs it; the layout gives `surfaces/` to
  CLI adapters, and R3 lets the CLI depend on a surface.
- **An off-the-shelf Mixpanel SDK** — rejected as ADR-0079 rejected one:
  `/track` is a single JSON POST over the workspace's existing HTTP client.
- **Reuse `relate-mcp`'s server for the serve path** — rejected: it is bound
  to a `Database` and three fixed tools; generalizing it to host one
  storeless tool costs more than the small loop.
- **Serve as an extension-program child** — rejected: it routes a
  first-party CLI feature through the extension install and trust
  lifecycle, and `feedback register` must point a client at the CLI binary
  itself.
- **Fold feedback into the telemetry module instead of a crate** — rejected:
  consent semantics differ (a person authorizing free-text reports versus
  anonymous counts), and the module is already at the size the crate split
  exists to prevent.
- **Build-time token injection, as the ported implementation's linker flags
  do** — rejected: nothing in the Rust release path injects a token, so a
  compile-time environment read would make a release build depend on the
  build host.
- **One endpoint policy for both surfaces** — rejected: tightening
  telemetry breaks its accepted override rule (ADR-0263), and loosening
  feedback would let an agent-adjacent path redirect free-text reports to
  any `https` host.

## Consequences

- New workspace member `surfaces/relate-feedback`; `cargo xtask
  check-boundaries` covers the new edge, and the CLI's dependency on it is
  one-way.
- `e2e_telemetry` passes unchanged against the moved client; telemetry's
  documentation, environment variables, and state layout stay as they are.
  The change is request-level visible: telemetry's POST now carries
  `ip=0` and follows no redirect, where the old one did neither.
- The CLI gains documented `feedback` verbs, two environment variables, and
  a hidden `serve` verb; `docs/api/cli/spec.md`, a runnable example, and the
  CLI skill bundle's reference land in the same change as the commands.
- Two Mixpanel projects must be provisioned — usage metrics (existing) and
  agent feedback (new) — each with a schema pinning the feedback event's
  properties.
- Reports are free text: only the validated, redacted payload leaves the
  machine, and a locally rejected report records no payload in its receipt.
