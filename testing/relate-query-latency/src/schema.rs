//! Upstream's schema (prrao87/graph-benchmark-ldbc `neo4j/build_graph.py`
//! at c8eb87f) as a table the loader expands (ADR-0243).
//!
//! One label per node file; the CSV `id` column becomes the INTEGER
//! property `ID`; every other loaded column keeps its header name; empty
//! cells are left out; dates and timestamps stay the files' ISO strings
//! (upstream's q8 runs `date(p.birthday)`); `length`, `classYear` and
//! `workFrom` are INTEGER. Relationship files are read by position (their
//! two id columns can share a name), with the third column, where there is
//! one, as the relationship's property. `person_email_emailaddress` and
//! `person_speaks_language` are not loaded (upstream leaves them out).

/// How a CSV cell becomes a property value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    Str,
    Int,
}

/// A node file: `<dataset>/<file>_0_0.csv`, header row, `|`-separated.
#[derive(Debug)]
pub struct NodeFile {
    /// Path under the data set without the `_0_0.csv` suffix.
    pub file: &'static str,
    pub label: &'static str,
    /// Loaded columns besides `id`, by header name.
    pub props: &'static [(&'static str, PropType)],
}

/// A relationship file: `<from id>|<to id>[|<prop>]`, header row skipped.
#[derive(Debug)]
pub struct RelFile {
    pub file: &'static str,
    pub rel_type: &'static str,
    pub from: &'static str,
    pub to: &'static str,
    /// The third column's property name and type, if the file has one.
    pub prop: Option<(&'static str, PropType)>,
}

use PropType::{Int, Str};

pub const NODE_FILES: &[NodeFile] = &[
    NodeFile {
        file: "static/organisation",
        label: "Organisation",
        props: &[("type", Str), ("name", Str), ("url", Str)],
    },
    NodeFile {
        file: "static/place",
        label: "Place",
        props: &[("name", Str), ("url", Str), ("type", Str)],
    },
    NodeFile {
        file: "static/tag",
        label: "Tag",
        props: &[("name", Str), ("url", Str)],
    },
    NodeFile {
        file: "static/tagclass",
        label: "Tagclass",
        props: &[("name", Str), ("url", Str)],
    },
    NodeFile {
        file: "dynamic/person",
        label: "Person",
        props: &[
            ("firstName", Str),
            ("lastName", Str),
            ("gender", Str),
            ("birthday", Str),
            ("creationDate", Str),
            ("locationIP", Str),
            ("browserUsed", Str),
        ],
    },
    NodeFile {
        file: "dynamic/forum",
        label: "Forum",
        props: &[("title", Str), ("creationDate", Str)],
    },
    NodeFile {
        file: "dynamic/post",
        label: "Post",
        props: &[
            ("imageFile", Str),
            ("creationDate", Str),
            ("locationIP", Str),
            ("browserUsed", Str),
            ("language", Str),
            ("content", Str),
            ("length", Int),
        ],
    },
    NodeFile {
        file: "dynamic/comment",
        label: "Comment",
        props: &[
            ("creationDate", Str),
            ("locationIP", Str),
            ("browserUsed", Str),
            ("content", Str),
            ("length", Int),
        ],
    },
];

const fn rel(
    file: &'static str,
    rel_type: &'static str,
    from: &'static str,
    to: &'static str,
    prop: Option<(&'static str, PropType)>,
) -> RelFile {
    RelFile {
        file,
        rel_type,
        from,
        to,
        prop,
    }
}

pub const REL_FILES: &[RelFile] = &[
    rel(
        "dynamic/forum_containerOf_post",
        "containerOf",
        "Forum",
        "Post",
        None,
    ),
    rel(
        "dynamic/comment_hasCreator_person",
        "commentHasCreator",
        "Comment",
        "Person",
        None,
    ),
    rel(
        "dynamic/post_hasCreator_person",
        "postHasCreator",
        "Post",
        "Person",
        None,
    ),
    rel(
        "dynamic/person_hasInterest_tag",
        "hasInterest",
        "Person",
        "Tag",
        None,
    ),
    rel(
        "dynamic/forum_hasMember_person",
        "hasMember",
        "Forum",
        "Person",
        Some(("joinDate", Str)),
    ),
    rel(
        "dynamic/forum_hasModerator_person",
        "hasModerator",
        "Forum",
        "Person",
        None,
    ),
    rel(
        "dynamic/comment_hasTag_tag",
        "commentHasTag",
        "Comment",
        "Tag",
        None,
    ),
    rel(
        "dynamic/forum_hasTag_tag",
        "forumHasTag",
        "Forum",
        "Tag",
        None,
    ),
    rel("dynamic/post_hasTag_tag", "postHasTag", "Post", "Tag", None),
    rel(
        "static/tag_hasType_tagclass",
        "hasType",
        "Tag",
        "Tagclass",
        None,
    ),
    rel(
        "dynamic/comment_isLocatedIn_place",
        "commentIsLocatedIn",
        "Comment",
        "Place",
        None,
    ),
    rel(
        "static/organisation_isLocatedIn_place",
        "organisationIsLocatedIn",
        "Organisation",
        "Place",
        None,
    ),
    rel(
        "dynamic/person_isLocatedIn_place",
        "personIsLocatedIn",
        "Person",
        "Place",
        None,
    ),
    rel(
        "dynamic/post_isLocatedIn_place",
        "postIsLocatedIn",
        "Post",
        "Place",
        None,
    ),
    rel(
        "static/place_isPartOf_place",
        "isPartOf",
        "Place",
        "Place",
        None,
    ),
    rel(
        "static/tagclass_isSubclassOf_tagclass",
        "isSubclassOf",
        "Tagclass",
        "Tagclass",
        None,
    ),
    rel(
        "dynamic/person_knows_person",
        "knows",
        "Person",
        "Person",
        Some(("creationDate", Str)),
    ),
    rel(
        "dynamic/person_likes_comment",
        "likeComment",
        "Person",
        "Comment",
        Some(("creationDate", Str)),
    ),
    rel(
        "dynamic/person_likes_post",
        "likePost",
        "Person",
        "Post",
        Some(("creationDate", Str)),
    ),
    rel(
        "dynamic/comment_replyOf_comment",
        "replyOfComment",
        "Comment",
        "Comment",
        None,
    ),
    rel(
        "dynamic/comment_replyOf_post",
        "replyOfPost",
        "Comment",
        "Post",
        None,
    ),
    rel(
        "dynamic/person_studyAt_organisation",
        "studyAt",
        "Person",
        "Organisation",
        Some(("classYear", Int)),
    ),
    rel(
        "dynamic/person_workAt_organisation",
        "workAt",
        "Person",
        "Organisation",
        Some(("workFrom", Int)),
    ),
];

/// The CSV path of a schema file under a data set directory.
pub fn csv_path(dataset: &std::path::Path, file: &str) -> std::path::PathBuf {
    dataset.join(format!("{file}_0_0.csv"))
}

/// The index of `label` in [`NODE_FILES`].
pub fn label_index(label: &str) -> Option<usize> {
    NODE_FILES.iter().position(|n| n.label == label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_upstream_counts_and_valid_endpoints() {
        assert_eq!(NODE_FILES.len(), 8);
        assert_eq!(REL_FILES.len(), 23);
        for r in REL_FILES {
            assert!(
                label_index(r.from).is_some(),
                "{}: from {}",
                r.rel_type,
                r.from
            );
            assert!(label_index(r.to).is_some(), "{}: to {}", r.rel_type, r.to);
        }
        let mut types: Vec<_> = REL_FILES.iter().map(|r| r.rel_type).collect();
        types.sort_unstable();
        types.dedup();
        assert_eq!(types.len(), 23, "relationship types are unique");
    }

    #[test]
    fn integer_columns_are_upstreams() {
        let ints: Vec<_> = NODE_FILES
            .iter()
            .flat_map(|n| n.props.iter())
            .chain(REL_FILES.iter().filter_map(|r| r.prop.as_ref()))
            .filter(|(_, t)| *t == PropType::Int)
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(ints, ["length", "length", "classYear", "workFrom"]);
    }
}
