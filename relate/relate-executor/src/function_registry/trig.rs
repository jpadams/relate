//! `Trigonometric` function rows: the radian-based trigonometric and
//! hyperbolic family, plus the degree/radian converters and the constants.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "acos",
        FunctionCategory::Trigonometric,
        "Returns the arccosine of a `FLOAT` in radians.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "asin",
        FunctionCategory::Trigonometric,
        "Returns the arcsine of a `FLOAT` in radians.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "atan",
        FunctionCategory::Trigonometric,
        "Returns the arctangent of a `FLOAT` in radians.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "atan2",
        FunctionCategory::Trigonometric,
        "Returns the arctangent2 of a set of coordinates in radians.",
        &[
            ProcArgMeta::described("y", "A y angle in radians.", "FLOAT"),
            ProcArgMeta::described("x", "An x angle in radians.", "FLOAT"),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "cos",
        FunctionCategory::Trigonometric,
        "Returns the cosine of a `FLOAT`.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "cosh",
        FunctionCategory::Trigonometric,
        "Returns the hyperbolic cosine of a `FLOAT`.",
        &[ProcArgMeta::described("input", "A value.", "FLOAT")],
        "FLOAT",
    ),
    FunctionMeta::described(
        "cot",
        FunctionCategory::Trigonometric,
        "Returns the cotangent of a `FLOAT`.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "coth",
        FunctionCategory::Trigonometric,
        "Returns the hyperbolic cotangent of a `FLOAT`.",
        &[ProcArgMeta::described("input", "A value.", "FLOAT")],
        "FLOAT",
    ),
    FunctionMeta::described(
        "degrees",
        FunctionCategory::Trigonometric,
        "Converts radians to degrees.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "haversin",
        FunctionCategory::Trigonometric,
        "Returns half the versine of a number.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "pi",
        FunctionCategory::Trigonometric,
        "Returns the mathematical constant pi.",
        &[],
        "FLOAT",
    ),
    FunctionMeta::described(
        "radians",
        FunctionCategory::Trigonometric,
        "Converts degrees to radians.",
        &[ProcArgMeta::described(
            "input",
            "An angle in degrees.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "sin",
        FunctionCategory::Trigonometric,
        "Returns the sine of a `FLOAT`.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "sinh",
        FunctionCategory::Trigonometric,
        "Returns the hyperbolic sine of a `FLOAT`.",
        &[ProcArgMeta::described("input", "A value.", "FLOAT")],
        "FLOAT",
    ),
    FunctionMeta::described(
        "tan",
        FunctionCategory::Trigonometric,
        "Returns the tangent of a `FLOAT`.",
        &[ProcArgMeta::described(
            "input",
            "An angle in radians.",
            "FLOAT",
        )],
        "FLOAT",
    ),
    FunctionMeta::described(
        "tanh",
        FunctionCategory::Trigonometric,
        "Returns the hyperbolic tangent of a `FLOAT`.",
        &[ProcArgMeta::described("input", "A value.", "FLOAT")],
        "FLOAT",
    ),
];
