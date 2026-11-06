//! The CSV renderer (RFC-4180 quoting lives in [`crate::escape`]).

use std::io::Write;

use indexmap::IndexMap;
use relate_model::CypherValue;

use crate::escape::csv_field;
use crate::Formatter;

impl Formatter {
    pub(crate) fn format_csv(
        &self,
        w: &mut dyn Write,
        columns: &[String],
        rows: &[IndexMap<String, CypherValue>],
    ) -> std::io::Result<()> {
        if self.headers {
            // Header cells get the same RFC-4180 quoting as values: a column
            // named `a,b` or one containing a quote/newline must not corrupt
            // the delimiter grid.
            let hdr: Vec<String> = columns.iter().map(|c| csv_field(c)).collect();
            writeln!(w, "{}", hdr.join(","))?;
        }
        for row in rows {
            let vals: Vec<String> = columns
                .iter()
                .map(|col| csv_field(&format_cell_csv(crate::cell(row, col))))
                .collect();
            writeln!(w, "{}", vals.join(","))?;
        }
        Ok(())
    }
}

/// Format a cell for CSV output — strings are unquoted (CSV handles its own quoting).
fn format_cell_csv(val: &CypherValue) -> String {
    match val {
        CypherValue::Null => String::new(),
        CypherValue::Str(s) => s.clone(),
        v => format!("{v}"),
    }
}
