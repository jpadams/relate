# Query-notification example

Run with `cargo run -p relate --example notifications`.

`QueryResult::notifications()` (ADR-0363) is the flat notification list
derived with the plan tree, stored on the result's `PlanArtifacts` beside it:
`EXPLAIN` and `PROFILE` populate it, every plain run leaves it empty because
no `PlanNode` tree is built there. Each record is a
`relate::Notification` naming its `relate::NotificationKind`; the kind supplies
the vocabulary row — code, GQLSTATUS, severity, category, title and canonical
remedy — while the record carries the instance `description`, an optional
instance `remedy` and, on operator-scoped kinds, the raising operator's
`PlanNode.id`.

The example explains a disjoint multi-pattern `MATCH`, whose
`CartesianProduct` plan warning projects to
`Neo.ClientNotification.Statement.CartesianProduct` (`03N90`,
`INFORMATION`, `PERFORMANCE`) with the warning's message as the description
and the warning's operator id. It then renders the two wire forms:

- `relate::notification_to_wire_json` — the engine-native
  form with `code`, `gql_status`, `severity`, `category`, `title`,
  `description`, `remedy` and `operator_id`, the shape the shared success
  envelope's `notifications` array carries;
- `relate::notification_to_neo4j_json` — Bolt's and the HTTP
  Query API v2's shape: `code`, `title`, `description`, `severity`,
  `category`, with the remedy appended to `description` as
  ` (remedy: <remedy>)` and no `remedy` or `position` key.

Finally it runs the same statement without `EXPLAIN` and asserts the list is
empty, pinning the opt-in budget: a plain query does no notification work.
