//! `relate clone [--wal-dir DIR] [--to-snapshot-format F] SOURCE DEST`
//! (ADR-0156, ADR-0282, ADR-0163): the full dir/`.rl` matrix, each side
//! classified by its `.rl` suffix.

use std::path::PathBuf;

use crate::parse_error::ParseError;
use relate::{CloneOptions, Database, MarkdownBodyProperties, SnapshotCodecKind};

/// Parsed `clone` command: the two positionals plus the engine's own
/// [`CloneOptions`] — no field-for-field mirror of it here.
#[derive(Debug, Clone)]
pub struct CloneCmd {
    pub source: String,
    pub dest: String,
    pub options: CloneOptions,
}

/// Parse the argument tail after the `clone` verb. A usage error is an `Err`
/// string the caller prints as the `clone` synopsis line plus a
/// `Run 'relate clone --help' for more information.` hint, then exits 2.
pub fn parse_clone_args(rest: &[String]) -> Result<CloneCmd, ParseError> {
    let mut wal_dir = None;
    let mut to_snapshot_format = None;
    let mut markdown_body_properties = None;
    let mut positionals = Vec::new();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wal-dir" => {
                let path = crate::parse_error::flag_value(&mut args, "--wal-dir", "a path")?;
                wal_dir = Some(PathBuf::from(path));
            }
            "--to-snapshot-format" => {
                let value = crate::parse_error::flag_value(
                    &mut args,
                    "--to-snapshot-format",
                    &format!("a value ({})", crate::help::compiled_codecs(" or ")),
                )?;
                // The same value parser the query command's
                // `--snapshot-format` uses (flags.rs): an uncompiled codec
                // parses here and fails closed in the engine, before the
                // destination is created (ADR-0163 §3). A repeated flag is
                // last-wins, like every other CLI value flag.
                to_snapshot_format = Some(
                    value
                        .parse::<SnapshotCodecKind>()
                        .map_err(|e| format!("invalid --to-snapshot-format: {e}"))?,
                );
            }
            "--markdown-body-properties" => {
                let value = crate::parse_error::flag_value(
                    &mut args,
                    "--markdown-body-properties",
                    "a value (property list, or Label=property list; entries separated by ';')",
                )?;
                // The same value parser the query command's flag uses
                // (flags.rs); a bad value is a usage error before the
                // destination is created.
                markdown_body_properties = Some(
                    MarkdownBodyProperties::parse(value)
                        .map_err(|e| format!("invalid --markdown-body-properties: {e}"))?,
                );
            }
            other if other.starts_with('-') => {
                return Err(ParseError::unexpected("clone", other));
            }
            other => positionals.push(other.to_string()),
        }
    }
    let mut positionals = positionals.into_iter();
    let (Some(source), Some(dest)) = (positionals.next(), positionals.next()) else {
        return Err("clone requires SOURCE and DEST".into());
    };
    if let Some(extra) = positionals.next() {
        return Err(ParseError::unexpected("clone", &extra));
    }
    Ok(CloneCmd {
        source,
        dest,
        options: CloneOptions {
            wal_dir,
            to_snapshot_format,
            markdown_body_properties,
        },
    })
}

/// Run `clone`. Returns the process exit code (0 success, 1 runtime).
pub fn run(cmd: CloneCmd) -> i32 {
    match Database::clone(&cmd.source, &cmd.dest, cmd.options) {
        Ok(()) => 0,
        Err(err) => {
            crate::print_error(format_args!("Failed to clone database: {err}"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    #[test]
    fn parse_to_snapshot_format_accepts_each_known_codec() {
        // `parquet` parses even without the `parquet` feature: the compiled
        // check is the engine's (ADR-0163 §3), like `--snapshot-format`.
        for (value, kind) in [
            ("json", SnapshotCodecKind::Json),
            ("parquet", SnapshotCodecKind::Parquet),
        ] {
            let cmd = parse_clone_args(&args(&["--to-snapshot-format", value, "a", "b"]))
                .unwrap_or_else(|e| panic!("{value}: {e}"));
            assert_eq!(cmd.options.to_snapshot_format, Some(kind), "{value}");
            assert_eq!(cmd.source, "a");
            assert_eq!(cmd.dest, "b");
        }
    }

    #[test]
    fn parse_to_snapshot_format_defaults_to_none() {
        let cmd = parse_clone_args(&args(&["a", "b"])).unwrap();
        assert_eq!(cmd.options.to_snapshot_format, None);
    }

    #[test]
    fn parse_to_snapshot_format_rejects_an_unknown_value() {
        let err = parse_clone_args(&args(&["--to-snapshot-format", "xml", "a", "b"]))
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("invalid --to-snapshot-format:"), "{err}");
        assert!(err.contains("unknown snapshot codec 'xml'"), "{err}");
    }

    #[test]
    fn parse_markdown_body_properties_accepts_and_rejects() {
        // The same grammar the query command's flag uses (flags.rs).
        let cmd = parse_clone_args(&args(&[
            "--markdown-body-properties",
            "summary,notes;Book=summary,review",
            "a",
            "b",
        ]))
        .unwrap();
        let config = cmd.options.markdown_body_properties.unwrap();
        assert_eq!(config.default, vec!["summary", "notes"]);
        assert_eq!(
            config.per_label.get("Book").unwrap(),
            &vec!["summary", "review"]
        );
        // A bad value is a usage error before the destination exists.
        let err = parse_clone_args(&args(&[
            "--markdown-body-properties",
            "summary,,notes",
            "a",
            "b",
        ]))
        .unwrap_err()
        .to_string();
        assert!(
            err.starts_with("invalid --markdown-body-properties:"),
            "{err}"
        );
    }

    #[test]
    fn parse_to_snapshot_format_requires_a_value() {
        let err = parse_clone_args(&args(&["a", "b", "--to-snapshot-format"]))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("--to-snapshot-format requires a value"),
            "{err}"
        );
    }
}
