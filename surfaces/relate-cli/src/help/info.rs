//! The `info` command help page: the shared synopsis plus its `--json` flag.

/// The `info` synopsis, shared by its help page and its usage errors.
pub(super) const INFO_USAGE: &str = "Usage: relate info --database PATH [--json]";

pub(super) fn info_help() -> String {
    format!(
        "{INFO_USAGE}

info prints store metadata for --database without opening it and exits.
  -d, --database PATH  Native directory or a .rl bundle; refused for :memory:.
  --json   Emit one JSON object on stdout (default: key/value table)
  Reports layout, codec, snapshot format version, size on disk,
  checkpoint-time node/rel counts (labeled with the checkpoint LSN),
  WAL segments past the checkpoint, and the index/constraint catalog.
  A JSON-codec store reports the codec envelope only; the format
  version, counts, indexes and constraints print as unavailable.
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{command_help, HelpScope, HelpTopic};

    #[test]
    fn info_help_lists_the_shared_synopsis_and_its_flag() {
        let help = command_help(HelpTopic::Info);
        crate::help::assert_single_usage(&help, INFO_USAGE);
        assert!(help.contains("--json"), "{help}");
        assert_eq!(HelpScope::Command(HelpTopic::Info).usage(), INFO_USAGE);
    }
}
