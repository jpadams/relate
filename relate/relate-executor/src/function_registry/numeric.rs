//! `Numeric` and `Logarithmic` function rows: the rounding/sign family the
//! corpus lists under `Numeric`, and the exponential/logarithmic family.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "abs",
        FunctionCategory::Numeric,
        "Returns the absolute value of an `INTEGER` or `FLOAT`.",
        &[
            ProcArgMeta::described(
                "input",
                "A numeric value from which the absolute number will be returned.",
                "INTEGER | FLOAT",
            ),
        ],
        "INTEGER | FLOAT",
    ),
    FunctionMeta::described(
        "ceil",
        FunctionCategory::Numeric,
        "Returns the smallest `FLOAT` that is greater than or equal to a number and equal to an `INTEGER`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value to be rounded to the nearest higher integer.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "ceiling",
        FunctionCategory::Numeric,
        "Returns the smallest `FLOAT` that is greater than or equal to a number and equal to an `INTEGER`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value to be rounded to the nearest higher integer.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "floor",
        FunctionCategory::Numeric,
        "Returns the largest `FLOAT` that is less than or equal to a number and equal to an `INTEGER`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value to be rounded to the nearest lower integer.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "isNaN",
        FunctionCategory::Numeric,
        "Returns whether the given `INTEGER` or `FLOAT` is NaN.",
        &[
            ProcArgMeta::described(
                "input",
                "A numeric value to be compared against `NaN`.",
                "INTEGER | FLOAT",
            ),
        ],
        "BOOLEAN",
    ),
    FunctionMeta::described(
        "rand",
        FunctionCategory::Numeric,
        "Returns a random `FLOAT` in the range from 0 (inclusive) to 1 (exclusive).",
        &[],
        "FLOAT",
    ),
    FunctionMeta::described(
        "round",
        FunctionCategory::Numeric,
        "Returns the value of a number rounded to the nearest `INTEGER`.",
        &[
            ProcArgMeta::described("value", "A value to be rounded.", "FLOAT"),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "round",
        FunctionCategory::Numeric,
        "Returns the value of a number rounded to the specified precision using rounding mode HALF_UP.",
        &[
            ProcArgMeta::described("value", "A value to be rounded.", "FLOAT"),
            ProcArgMeta::described("precision", "The rounding precision.", "INTEGER | FLOAT"),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "round",
        FunctionCategory::Numeric,
        "Returns the value of a number rounded to the specified precision with the specified rounding mode.",
        &[
            ProcArgMeta::described("value", "A value to be rounded.", "FLOAT"),
            ProcArgMeta::described("precision", "The rounding precision.", "INTEGER | FLOAT"),
            ProcArgMeta::described(
                "mode",
                "A precision rounding mode (`UP`, `DOWN`, `CEILING`, `FLOOR`, `HALF_UP`, `HALF_DOWN`, `HALF_EVEN`).",
                "STRING",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "sign",
        FunctionCategory::Numeric,
        "Returns the signum of an `INTEGER` or `FLOAT`: 0 if the number is 0, -1 for any negative number, and 1 for any positive number.",
        &[
            ProcArgMeta::described(
                "input",
                "A positive or negative number.",
                "INTEGER | FLOAT",
            ),
        ],
        "INTEGER",
    ),
    FunctionMeta::described(
        "e",
        FunctionCategory::Logarithmic,
        "Returns the base of the natural logarithm, e.",
        &[],
        "FLOAT",
    ),
    FunctionMeta::described(
        "exp",
        FunctionCategory::Logarithmic,
        "Returns e^n, where e is the base of the natural logarithm, and n is the value of the argument expression.",
        &[
            ProcArgMeta::described(
                "input",
                "A value to which the base of the natural logarithm, e, will be raised.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "log",
        FunctionCategory::Logarithmic,
        "Returns the natural logarithm of a `FLOAT`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value for which the natural logarithm will be returned.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "log10",
        FunctionCategory::Logarithmic,
        "Returns the common logarithm (base 10) of a `FLOAT`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value for which the common logarithm (base 10) will be returned.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "ln",
        FunctionCategory::Logarithmic,
        "Returns the natural logarithm of a `FLOAT`.",
        &[
            ProcArgMeta::described(
                "input",
                "A value for which the natural logarithm will be returned.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "sqrt",
        FunctionCategory::Logarithmic,
        "Returns the square root of a `FLOAT`.",
        &[
            ProcArgMeta::described(
                "input",
                "The value to calculate the square root of.",
                "FLOAT",
            ),
        ],
        "FLOAT",
    ),
];
