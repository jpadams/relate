//! `Aggregating` function rows: the aggregate catalogue, including the
//! CIP-128 snake_case aliases and the preview-gated GQL `stddev_*` spellings.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "avg",
        FunctionCategory::Aggregating,
        "Returns the average of a set of `INTEGER`, `FLOAT` or `DURATION` values.",
        &[
            ProcArgMeta::described(
                "input",
                "A value aggregated to form an average.",
                "INTEGER | FLOAT | DURATION",
            ),
        ],
        "INTEGER | FLOAT | DURATION",
    ),
    FunctionMeta::described(
        "collect",
        FunctionCategory::Aggregating,
        "Returns a list containing the values returned by an expression.",
        &[
            ProcArgMeta::described("input", "A value aggregated into a list.", "ANY"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "collect_list",
        FunctionCategory::Aggregating,
        "Returns a list containing the values returned by an expression.",
        &[
            ProcArgMeta::described("input", "A value aggregated into a list.", "ANY"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "count",
        FunctionCategory::Aggregating,
        "Returns the number of values or rows.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "ANY"),
        ],
        "INTEGER",
    ),
    FunctionMeta::described(
        "max",
        FunctionCategory::Aggregating,
        "Returns the maximum value in a set of values.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "ANY"),
        ],
        "ANY",
    ),
    FunctionMeta::described(
        "min",
        FunctionCategory::Aggregating,
        "Returns the minimum value in a set of values.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "ANY"),
        ],
        "ANY",
    ),
    FunctionMeta::described(
        "percentileCont",
        FunctionCategory::Aggregating,
        "Returns the percentile of a value over a group using linear interpolation.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "FLOAT"),
            ProcArgMeta::described("percentile", "A percentile between 0.0 and 1.0.", "FLOAT"),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "percentile_cont",
        FunctionCategory::Aggregating,
        "Returns the percentile of a value over a group using linear interpolation.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "FLOAT"),
            ProcArgMeta::described("percentile", "A percentile between 0.0 and 1.0.", "FLOAT"),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "percentileDisc",
        FunctionCategory::Aggregating,
        "Returns the nearest `INTEGER` or `FLOAT` value to the given percentile over a group using a rounding method.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "INTEGER | FLOAT"),
            ProcArgMeta::described("percentile", "A percentile between 0.0 and 1.0.", "FLOAT"),
        ],
        "INTEGER | FLOAT",
    ),
    FunctionMeta::described(
        "percentile_disc",
        FunctionCategory::Aggregating,
        "Returns the nearest `INTEGER` or `FLOAT` value to the given percentile over a group using a rounding method.",
        &[
            ProcArgMeta::described("input", "A value to be aggregated.", "INTEGER | FLOAT"),
            ProcArgMeta::described("percentile", "A percentile between 0.0 and 1.0.", "FLOAT"),
        ],
        "INTEGER | FLOAT",
    ),
    FunctionMeta::described(
        "stDev",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for a sample of a population.",
        &[
            ProcArgMeta::described(
                "input",
                "The value to calculate the standard deviation of.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "stdev_samp",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for a sample of a population.",
        &[
            ProcArgMeta::described(
                "input",
                "The value to calculate the standard deviation of.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "stDevP",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for an entire population.",
        &[
            ProcArgMeta::described(
                "input",
                "The value to calculate the population standard deviation of.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "stdev_pop",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for an entire population.",
        &[
            ProcArgMeta::described(
                "input",
                "The value to calculate the population standard deviation of.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "sum",
        FunctionCategory::Aggregating,
        "Returns the sum of a set of `INTEGER`, `FLOAT` or `DURATION` values",
        &[
            ProcArgMeta::described(
                "input",
                "A value to be aggregated.",
                "INTEGER | FLOAT | DURATION",
            ),
        ],
        "INTEGER | FLOAT | DURATION",
    ),
    FunctionMeta::described(
        "stddev_samp",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for a sample of a population.",
        &[ProcArgMeta::described("input", "A value to compare.", "FLOAT")],
        "FLOAT",
    ),
    FunctionMeta::described(
        "stddev_pop",
        FunctionCategory::Aggregating,
        "Returns the standard deviation for the given value over a group for an entire population.",
        &[ProcArgMeta::described("input", "A value to compare.", "FLOAT")],
        "FLOAT",
    ),
];
