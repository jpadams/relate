//! Reference-database queries (ADR-0144): run one plain Cypher statement on the Neo4j
//! EE database through the shared Query API v2 client (`relate-load`)
//! and return the raw `data.values` rows.

use relate_load::{Credentials, LoadError};
use serde_json::Value as Json;

/// Username of the reference container's single auth pair. Every
/// reference credential derives from this pair: the container boot env
/// (`NEO4J_AUTH`), the `relate pull` env-files, and the Query API v2
/// [`Credentials`] below.
pub const REFERENCE_USER: &str = "neo4j";

/// Password of the reference container's single auth pair — see
/// [`REFERENCE_USER`].
pub const REFERENCE_PASSWORD: &str = "referencepassword";

/// A reference-query failure, typed so the suite classifies on the Neo4j
/// error code instead of substring-sniffing a formatted message (F-6-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    /// The reference answered with a Query API v2 error object; `code` is
    /// its Neo4j status code (e.g. `Neo.ClientError.Statement.SyntaxError`)
    /// and `detail` the human-readable part.
    Query { code: String, detail: String },
    /// The local per-query timebox fired: the HTTP wait was abandoned, so
    /// there is no answer — and no trustworthy expected result (ADR-0176:
    /// inconclusive, never a diff). The abandoned statement keeps running
    /// on the reference; [`terminate_statement_transactions`] is the
    /// best-effort cleanup.
    TimedOut(String),
    /// No Neo4j error object: transport or body-shape failure.
    Other(String),
}

impl ReferenceError {
    /// The Neo4j status code, when the reference sent one.
    pub fn code(&self) -> Option<&str> {
        match self {
            ReferenceError::Query { code, .. } => Some(code),
            ReferenceError::TimedOut(_) | ReferenceError::Other(_) => None,
        }
    }
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceError::Query { code, detail } => write!(f, "{code}: {detail}"),
            ReferenceError::TimedOut(why) | ReferenceError::Other(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ReferenceError {}

impl From<LoadError> for ReferenceError {
    /// An HTTP error body carrying a Query API v2 error object
    /// (`{"errors":[{"code","message"}]}`) yields its Neo4j code; the
    /// envelope parse lives in the shared client ([`LoadError::query_api_error`]),
    /// everything else is `Other` with the client's message.
    fn from(e: LoadError) -> ReferenceError {
        if let (LoadError::Http { status, .. }, Some(api)) = (&e, e.query_api_error()) {
            return ReferenceError::Query {
                code: api.code,
                detail: format!("HTTP {status}: {}", api.message),
            };
        }
        ReferenceError::Other(format!("reference query failed: {e}"))
    }
}

/// Execute one statement on the reference database `database`; returns the
/// `data.values` rows (arrays of cells) on success. Every statement runs
/// under the per-query timebox (ADR-0144): the synchronous HTTP call cannot
/// be cancelled from the inside, so the wait is abandoned past the budget
/// and the error is the typed [`ReferenceError::TimedOut`]. Abandoning the
/// wait leaves the statement running on Neo4j (no server-wide
/// `db.transaction.timeout` is set — the multi-minute pull transaction must
/// never be cut off), so a timebox first issues a best-effort
/// [`terminate_statement_transactions`] for the abandoned statement. Other
/// failures are typed ([`ReferenceError`]): a reference answer carrying a
/// Query API v2 error object keeps its Neo4j status `code` so callers
/// classify on it, never on message text.
pub fn query(
    reference_uri: &str,
    database: &str,
    statement: &str,
) -> Result<Vec<Json>, ReferenceError> {
    let creds = database_credentials_uri(reference_uri, database);
    let statement = statement.to_string();
    let budget = crate::timebox::query_timeout();
    // `run_timed` is parameterized over the worker's error, so the typed
    // `ReferenceError` travels back in `RunError::Work` — only the worker-
    // panic case flattens to `Other`.
    match crate::timebox::run_timed(budget, "reference query", {
        let statement = statement.clone();
        move || {
            // Stream the rows (ADR-0158): a full-set query for the
            // LIMIT/SKIP subset check can exceed the buffered path's
            // 64 MiB cap on the larger datasets.
            let mut rows: Vec<Json> = Vec::new();
            relate_load::run_query_streaming(
                &creds,
                &statement,
                &Default::default(),
                true,
                &mut |batch: &[Json]| {
                    rows.extend_from_slice(batch);
                    Ok(())
                },
            )
            .map(|()| rows)
            .map_err(ReferenceError::from)
        }
    }) {
        Ok(rows) => Ok(rows),
        Err(crate::timebox::RunError::Work(e)) => Err(e),
        Err(crate::timebox::RunError::TimedOut(why)) => {
            terminate_statement_transactions(reference_uri, database, &statement);
            Err(ReferenceError::TimedOut(why))
        }
        Err(crate::timebox::RunError::Panicked(why)) => Err(ReferenceError::Other(why)),
    }
}

/// The Cypher 25 command [`terminate_statement_transactions`] runs: list
/// the database's transactions, keep the ones still executing the abandoned
/// statement verbatim, and terminate them. Matching on the exact
/// `currentQuery` text (never a substring) makes a false positive
/// structurally impossible against a differently-shaped statement, and the
/// per-database scoping of `SHOW TRANSACTIONS` keeps the other datasets'
/// concurrent transactions out of scope.
const TERMINATION_STATEMENT: &str = "SHOW TRANSACTIONS\n\
     YIELD transactionId, currentQuery\n\
     WHERE currentQuery = $statement\n\
     TERMINATE TRANSACTIONS transactionId";

/// Best-effort cleanup after a reference timebox: terminate the transaction
/// the abandoned statement is still running in, so it stops burning
/// reference CPU (and stops poisoning later queries with "Peer
/// disconnected" artifacts). This is deliberately NOT a server-wide
/// `db.transaction.timeout`: the pull runs one multi-minute transaction
/// that such a timeout would kill. A termination failure (the statement
/// already finished, the reference is wedged, …) is logged, never fatal.
pub(crate) fn terminate_statement_transactions(
    reference_uri: &str,
    database: &str,
    statement: &str,
) {
    let creds = database_credentials_uri(reference_uri, database);
    let mut params = serde_json::Map::new();
    params.insert(
        "statement".into(),
        serde_json::Value::String(statement.to_string()),
    );
    // The termination runs under the same per-query budget: a wedged
    // reference must not hang the dataset run on the cleanup path either.
    let result = crate::timebox::run_timed(
        crate::timebox::query_timeout(),
        "reference transaction termination",
        move || relate_load::run_query(&creds, TERMINATION_STATEMENT, &params, true),
    );
    if let Err(e) = result {
        eprintln!("reference: terminating the abandoned transaction failed (ignored): {e}");
    }
}

/// Query API v2 credentials for one database on the reference container.
pub fn database_credentials_uri(reference_uri: &str, database: &str) -> Credentials {
    Credentials {
        uri: reference_uri.to_string(),
        username: REFERENCE_USER.to_string(),
        password: REFERENCE_PASSWORD.to_string(),
        database: database.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_derive_from_shared_constants() {
        let creds = database_credentials_uri("http://localhost:7474", "movies");
        assert_eq!(creds.uri, "http://localhost:7474");
        assert_eq!(creds.username, REFERENCE_USER);
        assert_eq!(creds.password, REFERENCE_PASSWORD);
        assert_eq!(creds.database, "movies");
    }

    /// A Query API v2 error body yields the typed error with its Neo4j
    /// code (F-6-3): no formatted-message substring sniffing.
    #[test]
    fn http_error_body_parses_into_typed_code() {
        let body = r#"{"errors":[{"code":"Neo.ClientError.Statement.SyntaxError","message":"Invalid input 'WIP' (line 1, column 8)"}]}"#;
        let err = ReferenceError::from(LoadError::Http {
            status: 400,
            reply: body.to_string(),
        });
        assert_eq!(
            err.code(),
            Some("Neo.ClientError.Statement.SyntaxError"),
            "{err:?}"
        );
        assert!(err.to_string().contains("SyntaxError"), "{err}");
    }

    #[test]
    fn non_error_object_bodies_fall_back_to_other() {
        // Truncated / non-JSON body: no code, but the detail survives.
        let err = ReferenceError::from(LoadError::Http {
            status: 502,
            reply: "bad gateway".to_string(),
        });
        assert_eq!(err.code(), None, "{err:?}");
        assert!(err.to_string().contains("HTTP 502"), "{err}");
        // Non-HTTP client errors are Other too.
        let err = ReferenceError::from(LoadError::Io("connection refused".to_string()));
        assert_eq!(err.code(), None, "{err:?}");
        assert!(err.to_string().contains("connection refused"), "{err}");
    }

    /// The timebox is its own typed variant, not a string in `Other`:
    /// callers map it to the inconclusive outcome without message-sniffing.
    #[test]
    fn timed_out_is_a_typed_variant_without_a_code() {
        let err = ReferenceError::TimedOut("reference query exceeded the 20s timebox".to_string());
        assert_eq!(err.code(), None, "{err:?}");
        assert!(
            err.to_string().contains("exceeded the 20s timebox"),
            "{err}"
        );
    }

    /// The termination command is Cypher 25 `SHOW … TERMINATE TRANSACTIONS`,
    /// filtered on the abandoned statement's exact text via a parameter
    /// (never interpolated into the command text).
    #[test]
    fn termination_statement_filters_on_exact_query_text() {
        assert!(TERMINATION_STATEMENT.contains("SHOW TRANSACTIONS"));
        assert!(TERMINATION_STATEMENT.contains("YIELD transactionId, currentQuery"));
        assert!(TERMINATION_STATEMENT.contains("WHERE currentQuery = $statement"));
        assert!(TERMINATION_STATEMENT.contains("TERMINATE TRANSACTIONS transactionId"));
        // The statement text travels as a parameter: a hostile or
        // quote-bearing adoc statement cannot break out of the command.
        assert!(!TERMINATION_STATEMENT.contains("${statement}"));
    }
}
