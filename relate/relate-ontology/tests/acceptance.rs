//! Acceptance tests for the ontology document crate on the published
//! Ontology Graph Spec 1.1.0 shape (ADR-0364, raised by ADR-0487).
//!
//! The coverage follows the vendored schema clause by clause: the
//! exact-URI gate, the closed root, node/labels/property/relationship
//! entries, `RelationshipTarget`, the id-keyed constraint maps with the
//! uppercase `type` enum, the closed 39-token `Neo4jType` vocabulary, the
//! typed extension envelopes, the JSON/YAML round trips with key order
//! preserved, the rejection of every pre-bump shape, and the ADR-0226
//! graph-type derivation on spec-shape documents.

#[path = "acceptance/clauses.rs"]
mod clauses;
#[path = "acceptance/conversions.rs"]
mod conversions;
#[path = "acceptance/fixtures.rs"]
mod fixtures;
#[path = "acceptance/gate.rs"]
mod gate;
#[path = "acceptance/graph_type.rs"]
mod graph_type;
#[path = "acceptance/support.rs"]
mod support;
