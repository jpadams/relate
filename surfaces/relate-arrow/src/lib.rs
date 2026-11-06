//! Relate Arrow conversion utilities.
//!
//! Provides bidirectional conversion between CypherValue result sets and
//! Apache Arrow RecordBatch format, enabling zero-copy interop with
//! Arrow-native tools (Pandas, Polars, DuckDB, etc.).

pub mod aggregate;
pub mod from_arrow;
pub mod geo_encoding;
pub mod record_batch;
pub mod schema;
mod typed_json;
