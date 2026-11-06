//! Collection function rows: the `List`-category accessors, the `coll.*`
//! family (CIP-222/CIP-274/CIP-286), the element-wise conversions and
//! `reverse`, which the corpus classifies under `String`.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "labels",
        FunctionCategory::List,
        "Returns a `LIST<STRING>` containing the `STRING` representations for all the labels of a `NODE`.",
        &[
            ProcArgMeta::described("input", "A node whose labels will be returned.", "NODE"),
        ],
        "LIST<STRING>",
    ),
    FunctionMeta::described(
        "keys",
        FunctionCategory::List,
        "Returns a `LIST<STRING>` containing the `STRING` representations for all the property names of a `NODE`, `RELATIONSHIP` or `MAP`.",
        &[
            ProcArgMeta::described(
                "input",
                "A node or relationship from which the names of all properties will be returned.",
                "NODE | RELATIONSHIP | MAP",
            ),
        ],
        "LIST<STRING>",
    ),
    FunctionMeta::described(
        "tail",
        FunctionCategory::List,
        "Returns all but the first element in a `LIST<ANY>`.",
        &[
            ProcArgMeta::described(
                "input",
                "A list from which all but the first element will be returned.",
                "LIST<ANY>",
            ),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "range",
        FunctionCategory::List,
        "Returns a `LIST<INTEGER>` comprising all `INTEGER` values within a specified range.",
        &[
            ProcArgMeta::described("start", "The start value of the range.", "INTEGER"),
            ProcArgMeta::described("end", "The end value of the range.", "INTEGER"),
        ],
        "LIST<INTEGER>",
    ),
    FunctionMeta::described(
        "range",
        FunctionCategory::List,
        "Returns a `LIST<INTEGER>` comprising all `INTEGER` values within a specified range created with step length.",
        &[
            ProcArgMeta::described("start", "The start value of the range.", "INTEGER"),
            ProcArgMeta::described("end", "The end value of the range.", "INTEGER"),
            ProcArgMeta::described(
                "step",
                "The size of the increment (default value: 1).",
                "INTEGER",
            ),
        ],
        "LIST<INTEGER>",
    ),
    FunctionMeta::described(
        "nodes",
        FunctionCategory::List,
        "Returns a `LIST<NODE>` containing all the `NODE` values in a `PATH`.",
        &[
            ProcArgMeta::described("input", "A path whose nodes will be returned.", "PATH"),
        ],
        "LIST<NODE>",
    ),
    FunctionMeta::described(
        "relationships",
        FunctionCategory::List,
        "Returns a `LIST<RELATIONSHIP>` containing all the `RELATIONSHIP` values in a `PATH`.",
        &[
            ProcArgMeta::described(
                "input",
                "The path from which all relationships will be returned.",
                "PATH",
            ),
        ],
        "LIST<RELATIONSHIP>",
    ),
    FunctionMeta {
        name: "reduce",
        category: FunctionCategory::List,
        description: "Runs an expression against individual elements of a `LIST<ANY>`, storing the result of the expression in an accumulator.",
        special_signature: Some("(accumulator :: VARIABLE = initial :: ANY, variable :: VARIABLE IN list :: LIST<ANY> | expression :: ANY) :: ANY"),
        args: &[
            ProcArgMeta::described(
                "accumulator",
                "A variable that holds the result as the list is iterated.",
                "ANY",
            ),
            ProcArgMeta::described(
                "variable",
                "A variable that can be used within the reducing expression.",
                "LIST<ANY>",
            ),
        ],
        return_type: "ANY",
        deprecated_by: None,
    },
    FunctionMeta::described(
        "reverse",
        FunctionCategory::String,
        "Returns a `STRING` or `LIST<ANY>` in which the order of all characters or elements in the given `STRING` or `LIST<ANY>` have been reversed.",
        &[
            ProcArgMeta::described(
                "input",
                "The string or list to be reversed.",
                "STRING | LIST<ANY>",
            ),
        ],
        "STRING | LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.distinct",
        FunctionCategory::List,
        "Returns the given list with all duplicate values removed.",
        &[
            ProcArgMeta::described("list", "A list to be deduplicated.", "LIST<ANY>"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.flatten",
        FunctionCategory::List,
        "Returns a list flattened to the given nesting depth.",
        &[
            ProcArgMeta::described("list", "A nested list to be flattened.", "LIST<ANY>"),
            ProcArgMeta::described("depth", "The maximum depth to flatten to.", "INTEGER"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.flatten",
        FunctionCategory::List,
        "Returns a list flattened to ....",
        &[
            ProcArgMeta::described("list", "A list to flatten.", "LIST<ANY>"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.indexOf",
        FunctionCategory::List,
        "Returns the index for the first match of a value in the given list or -1 if the value is not present.",
        &[
            ProcArgMeta::described("list", "A list to be searched.", "LIST<ANY>"),
            ProcArgMeta::described("value", "A value to search for.", "ANY"),
        ],
        "INTEGER",
    ),
    FunctionMeta::described(
        "coll.insert",
        FunctionCategory::List,
        "Returns a list with the given value inserted at the given index.",
        &[
            ProcArgMeta::described("list", "A list to add to.", "LIST<ANY>"),
            ProcArgMeta::described("index", "The index to add the given value at.", "INTEGER"),
            ProcArgMeta::described("value", "The value to add into the list.", "ANY"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.remove",
        FunctionCategory::List,
        "Returns a list with the value at the given index removed.",
        &[
            ProcArgMeta::described("list", "A list to remove a value from.", "LIST<ANY>"),
            ProcArgMeta::described("index", "The index of the value to be removed.", "INTEGER"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta::described(
        "coll.max",
        FunctionCategory::List,
        "Returns the largest value present in the given list..",
        &[
            ProcArgMeta::described("list", "A list to be searched.", "LIST<ANY>"),
        ],
        "ANY",
    ),
    FunctionMeta::described(
        "coll.min",
        FunctionCategory::List,
        "Returns the smallest value present in the given list.",
        &[
            ProcArgMeta::described("list", "A list to be searched.", "LIST<ANY>"),
        ],
        "ANY",
    ),
    FunctionMeta::described(
        "coll.sort",
        FunctionCategory::List,
        "Returns a sorted list.",
        &[
            ProcArgMeta::described("list", "A list to be sorted.", "LIST<ANY>"),
        ],
        "LIST<ANY>",
    ),
    FunctionMeta {
        name: "coll.zip",
        category: FunctionCategory::List,
        description: "Returns the given lists zipped together.",
        special_signature: Some("(list1 :: LIST<ANY>, list2 :: LIST<ANY> ...) :: LIST<ANY>"),
        args: &[
            ProcArgMeta::described("list1", "A list to be zipped.", "LIST<ANY>"),
            ProcArgMeta::described("list2", "A list to be zipped.", "LIST<ANY>"),
        ],
        return_type: "LIST<ANY>",
        deprecated_by: None,
    },
    FunctionMeta::described(
        "coll.hasAnyOf",
        FunctionCategory::List,
        "Returns `true` if at least one element of the first list is present in the second list.",
        &[
            ProcArgMeta::described("list", "The list to search.", "LIST<ANY>"),
            ProcArgMeta::described("values", "The values to search for.", "LIST<ANY>"),
        ],
        "BOOLEAN",
    ),
    FunctionMeta::described(
        "coll.hasAllOf",
        FunctionCategory::List,
        "Returns `true` if all elements of the first list are present in the second list.",
        &[
            ProcArgMeta::described("list", "The list to search.", "LIST<ANY>"),
            ProcArgMeta::described("values", "The values to search for.", "LIST<ANY>"),
        ],
        "BOOLEAN",
    ),
    FunctionMeta::described(
        "coll.hasNoneOf",
        FunctionCategory::List,
        "Returns `true` if none of the elements of the first list are present in the second list.",
        &[
            ProcArgMeta::described("list", "The list to search.", "LIST<ANY>"),
            ProcArgMeta::described("values", "The values to search for.", "LIST<ANY>"),
        ],
        "BOOLEAN",
    ),
    FunctionMeta::described(
        "toStringList",
        FunctionCategory::List,
        "Converts a `LIST<ANY>` to a `LIST<STRING>` values. If any values are not convertible to `STRING` they will be null in the `LIST<STRING>` returned.",
        &[
            ProcArgMeta::described(
                "input",
                "A list of values to be converted into a list of strings.",
                "LIST<ANY>",
            ),
        ],
        "LIST<STRING>",
    ),
    FunctionMeta::described(
        "toFloatList",
        FunctionCategory::List,
        "Converts a `LIST<ANY> | VECTOR` to a `LIST<FLOAT>` values. If any values are not convertible to `FLOAT` they will be null in the `LIST<FLOAT>` returned.",
        &[
            ProcArgMeta::described(
                "input",
                "A list of values or vector to be converted into a list of floats.",
                "VECTOR | LIST<ANY>",
            ),
        ],
        "LIST<FLOAT>",
    ),
    FunctionMeta::described(
        "toIntegerList",
        FunctionCategory::List,
        "Converts a `LIST<ANY> | VECTOR` to a `LIST<INTEGER>` values. If any values are not convertible to `INTEGER` they will be null in the `LIST<INTEGER>` returned.",
        &[
            ProcArgMeta::described(
                "input",
                "A list of values or vector to be converted into a list of integers.",
                "VECTOR | LIST<ANY>",
            ),
        ],
        "LIST<INTEGER>",
    ),
    FunctionMeta::described(
        "toBooleanList",
        FunctionCategory::List,
        "Converts a `LIST<ANY>` of values to a `LIST<BOOLEAN>` values. If any values are not convertible to `BOOLEAN` they will be null in the `LIST<BOOLEAN>` returned.",
        &[
            ProcArgMeta::described(
                "input",
                "A list of values to be converted into a list of booleans.",
                "LIST<ANY>",
            ),
        ],
        "LIST<BOOLEAN>",
    ),
];
