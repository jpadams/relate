//! The parsed recipes as JSON, for the Node tools (`scripts/cookbook`): the
//! site build and the diagram renderer read recipes through this, so there
//! is one recipe parser ([`crate::recipe`]). `cookbook-export` prints it.

use serde_json::{json, Value};

use crate::frontmatter::Category;
use crate::recipe::{FenceKind, Recipe};

/// The export's format version; readers reject others.
pub const FORMAT: u64 = 1;

/// `{format, categories, inlineDataBytes, recipes}`.
pub fn export(recipes: &[Recipe]) -> Value {
    json!({
        "format": FORMAT,
        "categories": Category::ALL.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "inlineDataBytes": crate::INLINE_DATA_BYTES,
        "recipes": recipes.iter().map(recipe).collect::<Vec<_>>(),
    })
}

fn recipe(r: &Recipe) -> Value {
    json!({
        "slug": r.slug,
        "file": format!("{}.md", r.slug),
        "front": r.front.to_json(),
        "heading": r.heading,
        "bodyLine": r.body_line,
        "narrative": r.narrative(),
        "cells": r.cells.iter().map(|c| json!({
            "id": c.id,
            "code": c.code,
            "setup": c.setup,
            "params": c.params,
            "expectError": c.expect_error,
            "line": c.line,
            "result": c.result.as_ref().map(|res| json!({"kind": res.kind.info(), "text": res.text})),
        })).collect::<Vec<_>>(),
        "data": r.data.iter().map(|d| json!({"name": d.name, "text": d.text, "line": d.line})).collect::<Vec<_>>(),
        "diagrams": r.diagrams.iter().map(|d| json!({"index": d.index, "source": d.source, "line": d.line})).collect::<Vec<_>>(),
        "fences": r.fences.iter().map(|f| {
            let (kind, name) = match &f.kind {
                FenceKind::Cell(id) => ("cell", json!(id)),
                FenceKind::Result(id) => ("result", json!(id)),
                FenceKind::Data(name) => ("data", json!(name)),
                FenceKind::Diagram(index) => ("diagram", json!(index)),
                FenceKind::Code => ("code", Value::Null),
            };
            json!({"line": f.line, "kind": kind, "name": name})
        }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn exports_the_parse() {
        let src = "---\ntitle: T\nsummary: S\nlevel: Beginner\ncategory: Geospatial\nfeatures: [point]\nbrowser: full\n---\n\n# T\n\n```cypher cell=q\nRETURN 1\n```\n\n```table\nold\n```\n";
        let r = Recipe::parse("t", Path::new("t.md"), src.into()).unwrap();
        let out = export(&[r]);
        assert_eq!(out["format"], 1);
        assert_eq!(out["categories"][0], "Agent memory");
        let t = &out["recipes"][0];
        assert_eq!(t["front"]["level"], "Beginner");
        assert_eq!(t["bodyLine"], 9);
        assert_eq!(
            t["cells"][0]["result"],
            json!({"kind": "table", "text": "old"})
        );
        assert_eq!(
            t["fences"],
            json!([{"line": 12, "kind": "cell", "name": "q"}, {"line": 16, "kind": "result", "name": "q"}])
        );
    }
}
