use super::*;

/// A LOOKUP index's foreign `ON EACH labels(x)` variable is the
/// undefined-variable condition, so the tagged parse failure surfaces
/// `UndefinedVariable` from `kind()` — never the generic
/// `UnexpectedSyntax` fallback — while keeping the `SyntaxError` class
/// every parse failure has.
#[test]
fn lookup_index_foreign_variable_surfaces_undefined_variable() {
    let parse_error = relate_parser::parse("CREATE LOOKUP INDEX FOR (n) ON EACH labels(x)")
        .expect_err("a foreign variable is undefined in the lookup target");
    let error: EngineError = parse_error.into();
    assert_eq!(error.kind(), ErrorKind::UndefinedVariable);
    assert_eq!(error.tck_class(), TckErrorClass::SyntaxError);
}

/// A representative kind sample for the `EngineError` rendering tests. The
/// exhaustive per-kind tables (`as_str`/`from_name`/`tck_class` round
/// trips, the alias pair) moved with the vocabulary to
/// `relate_core::error_kinds` (ADR-0502 §2); these tests pin how
/// `EngineError` renders a kind, which is this crate's own contract.
const SAMPLE_KINDS: &[ErrorKind] = &[
    ErrorKind::SyntaxError,
    ErrorKind::UnexpectedSyntax,
    ErrorKind::UndefinedVariable,
    ErrorKind::UnknownFunction,
    ErrorKind::TypeError,
    ErrorKind::InvalidArgumentType,
    ErrorKind::ArgumentError,
    ErrorKind::NumberOutOfRange,
    ErrorKind::ParameterMissing,
    ErrorKind::EntityNotFound,
    ErrorKind::ConstraintViolation,
    ErrorKind::ProcedureNotFound,
    ErrorKind::DatabaseNotFound,
    ErrorKind::UnsupportedAdministrationCommand,
    ErrorKind::WriteOnReadOnlyAccessDatabase,
    ErrorKind::TransactionError,
];

#[test]
fn display_of_semantic_and_runtime_is_kind_colon_message() {
    for kind in SAMPLE_KINDS {
        let sem = EngineError::semantic(*kind, "the message");
        let run = EngineError::runtime(*kind, "the message");
        let expected = format!("{}: the message", kind.as_str());
        assert_eq!(sem.to_string(), expected);
        assert_eq!(run.to_string(), expected);
        assert_eq!(sem.kind(), *kind);
        assert_eq!(run.kind(), *kind);
    }
}

#[test]
fn display_of_uncategorized_has_no_prefix() {
    let err = EngineError::runtime(ErrorKind::Uncategorized, "raw legacy text: with colon");
    assert_eq!(err.to_string(), "raw legacy text: with colon");
    let err = EngineError::semantic(ErrorKind::Uncategorized, "plain");
    assert_eq!(err.to_string(), "plain");
}

/// The `ParameterMissing` kind keeps its canonical spelling in `Display`
/// (the TCK's `MissingParameter` alias lives only in the matcher-facing
/// `tck_detail`/`from_tck_detail` pair, in `relate_core::error_kinds`).
#[test]
fn parameter_missing_display_keeps_the_canonical_spelling() {
    assert_eq!(
        EngineError::semantic(ErrorKind::ParameterMissing, "boom").to_string(),
        "ParameterMissing: boom"
    );
}

#[test]
fn syntax_display_matches_parse_error_verbatim() {
    let pe = ParseError::new("unexpected token", 3, 7);
    let flattened = pe.to_string();
    let err: EngineError = pe.into();
    assert_eq!(err.to_string(), flattened);
    // An untagged parse error is the generic `UnexpectedSyntax` detail,
    // still of the `SyntaxError` TCK class.
    assert_eq!(err.kind(), ErrorKind::UnexpectedSyntax);
    assert_eq!(err.tck_class(), TckErrorClass::SyntaxError);
    assert!(err.to_string().starts_with("parse error: "));
}

#[test]
fn syntax_display_matches_parse_error_with_caret() {
    let pe = ParseError::new("boom", 1, 5).with_source("RETURN oops");
    let flattened = pe.to_string();
    let err: EngineError = pe.into();
    assert_eq!(err.to_string(), flattened);
}

#[test]
fn storage_display_matches_storage_error_verbatim() {
    let cases = [
        StorageError::NodeNotFound(42),
        StorageError::RelationshipNotFound(7),
        StorageError::NodeHasRelationships(1),
        StorageError::ConstraintViolation("dup name".to_string()),
        StorageError::IndexAlreadyExists("idx".to_string()),
        StorageError::IndexNotFound("idx".to_string()),
        StorageError::ConstraintAlreadyExists("c".to_string()),
        StorageError::ConstraintNotFound("c".to_string()),
        StorageError::TransactionsUnsupported,
        StorageError::TransactionAlreadyActive,
        StorageError::NoActiveTransaction,
        StorageError::PersistenceOutcomeIndeterminate("boom".to_string()),
        StorageError::Io {
            kind: std::io::ErrorKind::NotFound,
            message: "failed to read /db: No such file or directory".to_string(),
        },
        StorageError::Corruption("failed to parse JSON: expected value".to_string()),
        StorageError::Conflict("cannot flush while a transaction is active".to_string()),
        StorageError::Locked("/tmp/graph.db".to_string()),
        StorageError::ReadOnly("database is checked out at version 1".to_string()),
        StorageError::Other("misc io".to_string()),
    ];
    for se in cases {
        let flattened = se.to_string();
        let err: EngineError = se.clone().into();
        assert_eq!(err.to_string(), flattened, "storage Display drifted");
    }
}

#[test]
fn locked_display_names_the_path() {
    let err = StorageError::Locked("/tmp/graph.db".to_string());
    assert_eq!(
        err.to_string(),
        "database is locked by another process: /tmp/graph.db"
    );
}

#[test]
fn storage_kind_and_class_mapping() {
    assert_eq!(
        EngineError::Storage(StorageError::NodeNotFound(1)).kind(),
        ErrorKind::EntityNotFound
    );
    assert_eq!(
        EngineError::Storage(StorageError::ConstraintViolation("x".into())).kind(),
        ErrorKind::ConstraintViolation
    );
    assert_eq!(
        EngineError::Storage(StorageError::NodeHasRelationships(1)).kind(),
        ErrorKind::DeleteConnectedNode
    );
    // Both the connected-node and generic-constraint storage errors carry
    // the ConstraintVerificationFailed TCK class, but distinct details.
    assert_eq!(
        EngineError::Storage(StorageError::NodeHasRelationships(1)).tck_class(),
        TckErrorClass::ConstraintVerificationFailed
    );
    assert_eq!(
        EngineError::Storage(StorageError::IndexNotFound("i".into())).kind(),
        ErrorKind::SchemaError
    );
    assert_eq!(
        EngineError::Storage(StorageError::NoActiveTransaction).kind(),
        ErrorKind::TransactionError
    );
    assert_eq!(
        EngineError::Storage(StorageError::Other("x".into())).kind(),
        ErrorKind::Uncategorized
    );
    // New structured storage variants.
    assert_eq!(
        EngineError::Storage(StorageError::Io {
            kind: std::io::ErrorKind::NotFound,
            message: "failed to read".into(),
        })
        .kind(),
        ErrorKind::DatabaseError
    );
    assert_eq!(
        EngineError::Storage(StorageError::Corruption("bad json".into())).kind(),
        ErrorKind::DatabaseError
    );
    assert_eq!(
        EngineError::Storage(StorageError::Locked("/db".into())).kind(),
        ErrorKind::DatabaseError
    );
    assert_eq!(
        EngineError::Storage(StorageError::Conflict("txn".into())).kind(),
        ErrorKind::TransactionError
    );
    // The database-operational kinds fall under the SemanticError TCK class.
    assert_eq!(
        EngineError::Storage(StorageError::Locked("/db".into())).tck_class(),
        TckErrorClass::SemanticError
    );
    assert_eq!(
        EngineError::Storage(StorageError::Conflict("txn".into())).tck_class(),
        TckErrorClass::SemanticError
    );
    assert_eq!(
        EngineError::Storage(StorageError::NodeNotFound(1)).tck_class(),
        TckErrorClass::EntityNotFound
    );
}

#[test]
fn from_impls_wrap_verbatim() {
    let pe = ParseError::new("x", 1, 1);
    assert!(matches!(EngineError::from(pe), EngineError::Syntax(_)));
    let se = StorageError::Other("y".into());
    assert!(matches!(EngineError::from(se), EngineError::Storage(_)));
}

/// A parse error tagged with a detail surfaces that finer kind through
/// `kind()`/`tck_class()` while `Display` stays byte-for-byte identical to
/// the untagged parse error (the prime invariant).
#[test]
fn tagged_parse_error_surfaces_detail_without_changing_display() {
    // Integer overflow.
    let untagged = ParseError::new("integer overflow: 9223372036854775808", 1, 8);
    let tagged = ParseError::new("integer overflow: 9223372036854775808", 1, 8)
        .with_detail("IntegerOverflow");
    let untagged_err: EngineError = untagged.into();
    let tagged_err: EngineError = tagged.into();
    assert_eq!(untagged_err.to_string(), tagged_err.to_string());
    // Untagged → the generic `UnexpectedSyntax` detail.
    assert_eq!(untagged_err.kind(), ErrorKind::UnexpectedSyntax);
    assert_eq!(tagged_err.kind(), ErrorKind::IntegerOverflow);
    assert_eq!(tagged_err.tck_class(), TckErrorClass::SyntaxError);

    // Floating-point overflow.
    let fp: EngineError =
        ParseError::new("FloatingPointOverflow: float literal is too large", 1, 8)
            .with_detail("FloatingPointOverflow")
            .into();
    assert_eq!(fp.kind(), ErrorKind::FloatingPointOverflow);
    assert_eq!(fp.tck_class(), TckErrorClass::SyntaxError);

    // An unrecognized/garbage tag falls back to the generic
    // `UnexpectedSyntax` kind (SyntaxError class), never panics.
    let bogus: EngineError = ParseError::new("boom", 1, 1)
        .with_detail("NotARealKind")
        .into();
    assert_eq!(bogus.kind(), ErrorKind::UnexpectedSyntax);
    assert_eq!(bogus.tck_class(), TckErrorClass::SyntaxError);
}
