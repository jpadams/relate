//! GQL temporal alias spellings (CIP-245).

use super::super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(crate) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "local_time",
        FunctionCategory::Temporal,
        "Creates a `LOCAL TIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('hour, 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond' as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "LOCAL TIME",
    ),
    FunctionMeta::described(
        "local_time",
        FunctionCategory::Temporal,
        "Creates a `LOCAL TIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('hour, 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond' as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
            ProcArgMeta::with_default(
                "pattern",
                "A pattern used to parse a local time value. If the pattern is not provided the value will be parsed according to default patterns.",
                "STRING",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "LOCAL TIME",
    ),
    FunctionMeta::described(
        "local_datetime",
        FunctionCategory::Temporal,
        "Creates a `LOCAL DATETIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('year', 'month', 'day', 'hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "LOCAL DATETIME",
    ),
    FunctionMeta::described(
        "local_datetime",
        FunctionCategory::Temporal,
        "Creates a `LOCAL DATETIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('year', 'month', 'day', 'hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
            ProcArgMeta::with_default(
                "pattern",
                "A pattern used to parse a local datetime value. If the pattern is not provided the value will be parsed according to default patterns.",
                "STRING",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "LOCAL DATETIME",
    ),
    FunctionMeta::described(
        "zoned_time",
        FunctionCategory::Temporal,
        "Creates a `ZONED TIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond', 'timezone') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "ZONED TIME",
    ),
    FunctionMeta::described(
        "zoned_time",
        FunctionCategory::Temporal,
        "Creates a `ZONED TIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond', 'timezone') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
            ProcArgMeta::with_default(
                "pattern",
                "A pattern used to parse a zoned time value. If the pattern is not provided the value will be parsed according to default patterns.",
                "STRING",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "ZONED TIME",
    ),
    FunctionMeta::described(
        "zoned_datetime",
        FunctionCategory::Temporal,
        "Creates a `ZONED DATETIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('year', 'month', 'day', 'hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond', 'timezone', 'epochSeconds', 'epochMillis') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "ZONED DATETIME",
    ),
    FunctionMeta::described(
        "zoned_datetime",
        FunctionCategory::Temporal,
        "Creates a `ZONED DATETIME` instant.",
        &[
            ProcArgMeta::with_default(
                "input",
                "Either a string representation of a temporal value, a map containing the single key 'timezone', or a map containing temporal values ('year', 'month', 'day', 'hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond', 'timezone', 'epochSeconds', 'epochMillis') as components.",
                "ANY",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
            ProcArgMeta::with_default(
                "pattern",
                "A pattern used to parse a zoned datetime value. If the pattern is not provided the value will be parsed according to default patterns.",
                "STRING",
                "DEFAULT_TEMPORAL_ARGUMENT",
            ),
        ],
        "ZONED DATETIME",
    ),
];
