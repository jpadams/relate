//! The `clone` command help page: the shared synopsis plus its flags.

/// The `clone` synopsis, shared by its help page and its usage errors.
pub(super) const CLONE_USAGE: &str = "Usage: relate clone [--wal-dir DIR] \
[--to-snapshot-format F] [--markdown-body-properties LIST] SOURCE DEST";

pub(super) fn clone_help() -> String {
    format!(
        "{CLONE_USAGE}

clone copies a database into a new store and exits. Each side is a
native directory, or a single-file bundle when its path ends in .rl.
It reads the LSN visible on open, does not follow, and does not write
the source. Refused for :memory:. --wal-dir is the source WAL
(directory sources only; a .rl bundle embeds its WAL); the clone's
WAL is <DEST>/wal for a directory destination.
  --wal-dir DIR           Source WAL of a directory source (default <SOURCE>/wal)
  --to-snapshot-format F  Destination snapshot codec ({}); default: the source codec
  --markdown-body-properties LIST  Body properties of a markdown destination
                         (LIST or Label=LIST; entries separated by ';'); default:
                         the source's recorded configuration, else body
",
        super::compiled_codecs("|")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{command_help, HelpTopic};

    #[test]
    fn clone_help_lists_the_shared_synopsis_and_its_flags() {
        let help = command_help(HelpTopic::Clone);
        crate::help::assert_single_usage(&help, CLONE_USAGE);
        for present in [
            "--wal-dir",
            "--to-snapshot-format",
            "--markdown-body-properties",
            "Refused for :memory:",
        ] {
            assert!(help.contains(present), "{present:?} missing:\n{help}");
        }
    }
}
