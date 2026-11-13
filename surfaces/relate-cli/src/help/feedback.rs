//! The `feedback` command help page: the shared synopsis plus its actions and
//! `submit`'s flags.

/// The `feedback` synopsis, shared by its help page and its usage errors.
pub(super) const FEEDBACK_USAGE: &str =
    "Usage: relate feedback submit|enable|disable|status [FLAGS]";

pub(super) fn feedback_help() -> String {
    format!(
        "{FEEDBACK_USAGE}

feedback sends a person-consented report about this CLI to the maintainers.
  submit   Send one person-consented feedback report
  enable   Authorize feedback reports for 30 days
  disable  Revoke the feedback authorization
  status   Print whether feedback reporting is enabled

feedback submit flags:
  --category NAME     Required; one of unclear_error, unexpected_behavior,
                      missing_capability, or other_friction
  --expected TEXT     Required; what you expected, non-blank and at most
                      255 bytes (a longer value is rejected, never truncated)
  --observed TEXT     Required; what actually happened, the same rules as
                      --expected
  --command PATH      The CLI command path; kept only when it names a
                      visible command, else dropped and named in
                      dropped_fields
  --error-code CODE   The CLI error code; kept only when it is one of the
                      exit table's five nonzero code names, else dropped
                      and named in dropped_fields
  --model MODEL       The agent model label: lowercase, at most 40
                      characters, a provider prefix or an o<digit> family
                      name; else dropped and named in dropped_fields
  --request-id UUID   A lowercase UUIDv4 request id; an invalid value is a
                      violation and a fresh id is used (the same id can be
                      resent after failed or unknown)
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{command_help, HelpScope, HelpTopic};

    #[test]
    fn feedback_help_lists_the_shared_synopsis_its_actions_and_submit_flags() {
        let help = command_help(HelpTopic::Feedback);
        crate::help::assert_single_usage(&help, FEEDBACK_USAGE);
        for needle in [
            "submit",
            "enable",
            "disable",
            "status",
            "--category",
            "--expected",
            "--observed",
            "--command",
            "--error-code",
            "--model",
            "--request-id",
        ] {
            assert!(help.contains(needle), "{needle:?} missing:\n{help}");
        }
        assert_eq!(
            HelpScope::Command(HelpTopic::Feedback).usage(),
            FEEDBACK_USAGE
        );
    }
}
