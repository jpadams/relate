//! Vector function rows: the `vector` constructor (which the corpus
//! classifies under `Scalar`), its dimension accessor and the
//! similarity/distance family (CIP-116, CIP-121).

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "vector",
        FunctionCategory::Scalar,
        "Constructs a `VECTOR` value.",
        &[
            ProcArgMeta::described(
                "vectorValue",
                "The numeric values to create the vector coordinates from.",
                "STRING | LIST<INTEGER | FLOAT>",
            ),
            ProcArgMeta::described(
                "dimension",
                "The dimension (number of coordinates) of the vector.",
                "INTEGER",
            ),
            ProcArgMeta::described(
                "coordinateType",
                "The type of each coordinate in the vector.",
                "[INTEGER64, INTEGER32, INTEGER16, INTEGER8, FLOAT64, FLOAT32]",
            ),
        ],
        "VECTOR",
    ),
    FunctionMeta::described(
        "vector_dimension_count",
        FunctionCategory::Vector,
        "Returns the dimension of a `VECTOR`.",
        &[
            ProcArgMeta::described(
                "input",
                "The vector to calculate the dimension of.",
                "VECTOR",
            ),
        ],
        "INTEGER",
    ),
    FunctionMeta::described(
        "vector.similarity.cosine",
        FunctionCategory::Vector,
        "Returns a `FLOAT` representing the similarity between the argument vectors based on their cosine.",
        &[
            ProcArgMeta::described(
                "a",
                "A vector or list value representing the first vector.",
                "VECTOR | LIST<INTEGER | FLOAT>",
            ),
            ProcArgMeta::described(
                "b",
                "A vector or list value representing the second vector.",
                "VECTOR | LIST<INTEGER | FLOAT>",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "vector.similarity.euclidean",
        FunctionCategory::Vector,
        "Returns a `FLOAT` representing the similarity between the argument vectors based on their Euclidean distance.",
        &[
            ProcArgMeta::described(
                "a",
                "A vector or list value representing the first vector.",
                "VECTOR | LIST<INTEGER | FLOAT>",
            ),
            ProcArgMeta::described(
                "b",
                "A vector or list value representing the second vector.",
                "VECTOR | LIST<INTEGER | FLOAT>",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "vector_distance",
        FunctionCategory::Vector,
        "Returns a `FLOAT` representing the distance between the two vector values based on the selected `vectorDistanceMetric` algorithm.",
        &[
            ProcArgMeta::described("vector1", "The first vector.", "VECTOR"),
            ProcArgMeta::described("vector2", "The second vector.", "VECTOR"),
            ProcArgMeta::described(
                "coordinateType",
                "The vector distance algorithm to calculate the distance by.",
                "[EUCLIDEAN, EUCLIDEAN_SQUARED, MANHATTAN, COSINE, DOT, HAMMING]",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "vector_norm",
        FunctionCategory::Vector,
        "Returns a `FLOAT` representing the distance between the given vector and an origin vector, which is a vector with the same dimension with all coordinates set to zero, calculated using the specified `vectorDistanceMetric`.",
        &[
            ProcArgMeta::described(
                "vector",
                "A vector for which the norm to the origin vector will be computed.",
                "VECTOR",
            ),
            ProcArgMeta::described(
                "vectorDistanceMetric",
                "The vector distance algorithm to calculate the distance by.",
                "[EUCLIDEAN, MANHATTAN]",
            ),
        ],
        "FLOAT",
    ),
];
