//! `String` function rows: text manipulation, normalization and the
//! `string.*` family.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "normalize",
        FunctionCategory::String,
        "Normalize a `STRING`. The `STRING` will be normalized using the normal form 'NFC'.",
        &[
            ProcArgMeta::described("input", "A value to be normalized.", "STRING"),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "normalize",
        FunctionCategory::String,
        "Normalize a `STRING`. The `STRING` will be normalized according to the specified normalization form.",
        &[
            ProcArgMeta::described("input", "A value to be normalized.", "STRING"),
            ProcArgMeta::with_default(
                "normalForm",
                "A keyword specifying any of the normal forms; NFC, NFD, NFKC or NFKD.",
                "[NFC, NFD, NFKC, NFKD]",
                "NFC",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "toString",
        FunctionCategory::String,
        "Converts a value to a `STRING`.",
        &[
            ProcArgMeta::described("input", "A value to be converted into a string.", "ANY"),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "toStringOrNull",
        FunctionCategory::String,
        "Converts a value to a `STRING` using the same conversion rules as `toString()`, returning null instead of raising an error for any input it cannot convert.",
        &[
            ProcArgMeta::described(
                "input",
                "A value to be converted into a string or null.",
                "ANY",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "toLower",
        FunctionCategory::String,
        "Returns the given `STRING` in lowercase.",
        &[
            ProcArgMeta::described(
                "input",
                "A string to be converted into lowercase.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "toUpper",
        FunctionCategory::String,
        "Returns the given `STRING` in uppercase.",
        &[
            ProcArgMeta::described(
                "input",
                "A string to be converted into uppercase.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "upper",
        FunctionCategory::String,
        "Returns the given `STRING` in uppercase.",
        &[
            ProcArgMeta::described(
                "input",
                "A string to be converted into uppercase.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "lower",
        FunctionCategory::String,
        "Returns the given `STRING` in lowercase.",
        &[
            ProcArgMeta::described(
                "input",
                "A string to be converted into lowercase.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "trim",
        FunctionCategory::String,
        "Returns the given `STRING` with leading and trailing whitespace removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which all leading and trailing whitespace will be removed.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta {
        name: "trim",
        category: FunctionCategory::String,
        description: "Returns the given `STRING` with leading and/or trailing `trimCharacterString` removed.",
        special_signature: Some("([[LEADING | TRAILING | BOTH] [trimCharacterString :: STRING] FROM] input :: STRING) :: STRING"),
        args: &[
            ProcArgMeta::described(
                "trimSpecification",
                "The parts of the string to trim; LEADING, TRAILING, BOTH",
                "[LEADING, TRAILING, BOTH]",
            ),
            ProcArgMeta::described(
                "trimCharacterString",
                "The characters to be removed from the start and/or end of the given string.",
                "STRING",
            ),
            ProcArgMeta::described(
                "input",
                "A value from which all leading and/or trailing trim characters will be removed.",
                "STRING",
            ),
        ],
        return_type: "STRING",
        deprecated_by: None,
    },
    FunctionMeta::described(
        "ltrim",
        FunctionCategory::String,
        "Returns the given `STRING` with leading whitespace removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value in which all leading whitespace will be removed.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "ltrim",
        FunctionCategory::String,
        "Returns the given `STRING` with leading `trimCharacterString` characters removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which the leading trim character will be removed.",
                "STRING",
            ),
            ProcArgMeta::described(
                "trimCharacterString",
                "A character to be removed from the start of the given string.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "rtrim",
        FunctionCategory::String,
        "Returns the given `STRING` with trailing whitespace removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which all leading and trailing whitespace will be removed.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "rtrim",
        FunctionCategory::String,
        "Returns the given `STRING` with trailing `trimCharacterString` characters removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which the leading and trailing trim character will be removed.",
                "STRING",
            ),
            ProcArgMeta::described(
                "trimCharacterString",
                "A character to be removed from the start and end of the given string.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "btrim",
        FunctionCategory::String,
        "Returns the given `STRING` with leading and trailing whitespace removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which all leading and trailing whitespace will be removed.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "btrim",
        FunctionCategory::String,
        "Returns the given `STRING` with leading and trailing `trimCharacterString` characters removed.",
        &[
            ProcArgMeta::described(
                "input",
                "A value from which the leading and trailing trim character will be removed.",
                "STRING",
            ),
            ProcArgMeta::described(
                "trimCharacterString",
                "A character to be removed from the start and end of the given string.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "replace",
        FunctionCategory::String,
        "Returns a `STRING` in which all occurrences of a specified search `STRING` in the given `STRING` have been replaced by another (specified) replacement `STRING`.",
        &[
            ProcArgMeta::described("original", "The string to be modified.", "STRING"),
            ProcArgMeta::described(
                "search",
                "The value to replace in the original string.",
                "STRING",
            ),
            ProcArgMeta::described(
                "replace",
                "The value to be inserted in the original string.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "replace",
        FunctionCategory::String,
        "Returns a `STRING` in which all occurrences of a specified search `STRING` in the given `STRING` have been replaced by another (specified) replacement `STRING`.",
        &[
            ProcArgMeta::described("original", "The string to be modified.", "STRING"),
            ProcArgMeta::described(
                "search",
                "The value to replace in the original string.",
                "STRING",
            ),
            ProcArgMeta::described(
                "replace",
                "The value to be inserted in the original string.",
                "STRING",
            ),
            ProcArgMeta::described(
                "limit",
                "The maximum amount of times the search value should be replaced in the string, starting from the left.",
                "INTEGER",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "substring",
        FunctionCategory::String,
        "Returns a substring of the given `STRING`, beginning with a 0-based index start.",
        &[
            ProcArgMeta::described("original", "The string to be shortened.", "STRING"),
            ProcArgMeta::described("start", "The start position of the new string.", "INTEGER"),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "substring",
        FunctionCategory::String,
        "Returns a substring of a given `length` from the given `STRING`, beginning with a 0-based index start.",
        &[
            ProcArgMeta::described("original", "The string to be shortened.", "STRING"),
            ProcArgMeta::described("start", "The start position of the new string.", "INTEGER"),
            ProcArgMeta::described("length", "The length of the new string.", "INTEGER"),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "split",
        FunctionCategory::String,
        "Returns a `LIST<STRING>` resulting from the splitting of the given `STRING` around matches of the given delimiter(s).",
        &[
            ProcArgMeta::described("original", "The string to be split.", "STRING"),
            ProcArgMeta::described(
                "splitDelimiters",
                "The string with which to split the original string.",
                "STRING | LIST<STRING>",
            ),
        ],
        "LIST<STRING>",
    ),
    FunctionMeta::described(
        "left",
        FunctionCategory::String,
        "Returns a `STRING` containing the specified number (`INTEGER`) of leftmost characters in the given `STRING`.",
        &[
            ProcArgMeta::described(
                "original",
                "A string value whose rightmost characters will be trimmed.",
                "STRING",
            ),
            ProcArgMeta::described(
                "length",
                "The length of the leftmost characters to be returned.",
                "INTEGER",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "right",
        FunctionCategory::String,
        "Returns a `STRING` containing the specified number of rightmost characters in the given `STRING`.",
        &[
            ProcArgMeta::described(
                "original",
                "A string value whose leftmost characters will be trimmed.",
                "STRING",
            ),
            ProcArgMeta::described(
                "length",
                "The length of the rightmost characters to be returned.",
                "INTEGER",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "string.join",
        FunctionCategory::String,
        "Concatenates the elements in `input`, inserting `delimiter` between each element, and returns the resulting `STRING`.",
        &[
            ProcArgMeta::described("input", "A list of strings to be joined.", "LIST<STRING>"),
            ProcArgMeta::described("delimiter", "The joining delimiter", "STRING"),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "string.regexReplace",
        FunctionCategory::String,
        "Returns a `STRING` where all matches of `regex` in the `original` `STRING` are replaced with the `replacement` `STRING`.",
        &[
            ProcArgMeta::described("original", "The string to be modified.", "STRING"),
            ProcArgMeta::described(
                "regex",
                "The regex pattern to replace in the original `STRING`.",
                "STRING",
            ),
            ProcArgMeta::described(
                "replacement",
                "The value to be inserted in the original string.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "string.indexOf",
        FunctionCategory::String,
        "Returns the index of the first occurrence of the given `value` within the `input` `STRING`, or -1 if no match is found.",
        &[
            ProcArgMeta::described("input", "The string to be searched.", "STRING"),
            ProcArgMeta::described("value", "A value to search for.", "STRING"),
        ],
        "INTEGER",
    ),
];
