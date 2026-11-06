//! `Database` and `Graph` function rows: `db.nameFromElementId` and the
//! composite-database `graph.*` selectors (CIP-230's reserved namespace).

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "db.nameFromElementId",
        FunctionCategory::Database,
        "Resolves the database name for the given element id",
        &[
            ProcArgMeta::described(
                "elementId",
                "An element id of a node or relationship.",
                "STRING",
            ),
        ],
        "STRING",
    ),
    FunctionMeta::described(
        "graph.names",
        FunctionCategory::Graph,
        "Lists the names of graphs in the current database.",
        &[],
        "LIST<STRING>",
    ),
    FunctionMeta::described(
        "graph.byName",
        FunctionCategory::Graph,
        "Returns the graph reference of the given name. It is only supported in the `USE` clause, on composite databases.",
        &[
            ProcArgMeta::described("name", "The name of the graph to be resolved.", "STRING"),
        ],
        "GRAPH",
    ),
    FunctionMeta::described(
        "graph.byElementId",
        FunctionCategory::Graph,
        "Returns the graph reference with the given element id. It is only supported in the `USE` clause, on composite databases.",
        &[
            ProcArgMeta::described(
                "elementId",
                "An element id of a node or relationship.",
                "STRING",
            ),
        ],
        "GRAPH",
    ),
    FunctionMeta::described(
        "graph.propertiesByName",
        FunctionCategory::Graph,
        "Returns the `MAP` of properties associated with a graph.",
        &[
            ProcArgMeta::described(
                "graphName",
                "The name of the graph from which all associated properties will be returned.",
                "STRING",
            ),
        ],
        "MAP",
    ),
];
