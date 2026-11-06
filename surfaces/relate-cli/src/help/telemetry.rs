//! The `telemetry` command help page: the shared synopsis plus its actions.

/// The `telemetry` synopsis, shared by its help page and its usage errors.
pub(super) const TELEMETRY_USAGE: &str = "Usage: relate telemetry on|off|status";

pub(super) fn telemetry_help() -> String {
    format!(
        "{TELEMETRY_USAGE}

telemetry manages anonymous usage metrics.
  on      Enable and persist the setting
  off     Disable and persist the setting
  status  Print the effective state and the layer that decided it
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{command_help, HelpTopic};

    #[test]
    fn telemetry_help_lists_the_shared_synopsis_and_its_actions() {
        let help = command_help(HelpTopic::Telemetry);
        crate::help::assert_single_usage(&help, TELEMETRY_USAGE);
        for action in ["on", "off", "status"] {
            assert!(help.contains(action), "{action:?} missing:\n{help}");
        }
    }
}
