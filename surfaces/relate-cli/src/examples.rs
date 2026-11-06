//! Examples in `--help` (ADR-0197): the example type, the opt-out environment
//! variable, and the rendering of the `Examples:` section. The per-command
//! examples live with the command table in [`crate::help`]; the pair on the
//! top-level page lives here.

/// One help example: what it does, and the command line.
pub(crate) type Example = (&'static str, &'static str);

/// The environment variable that hides the `Examples:` section.
pub(crate) const ENV: &str = "RELATE_HELP_EXAMPLES";

/// The two examples on the top-level page: the common read, and the safe way
/// for an agent to explore a database.
pub(crate) const TOP_LEVEL: &[Example] = &[
    (
        "Run a query and print JSON",
        "relate --database ./mydata --json -c \"MATCH (n) RETURN count(n) AS nodes\"",
    ),
    (
        "Explore without being able to change anything",
        "relate --database ./mydata --read-only --mode envelope -c \"MATCH (n:Person) RETURN n.name LIMIT 5\"",
    ),
];

/// Whether help pages show their examples, from the value of
/// `RELATE_HELP_EXAMPLES`: on unless it is `0`, `false`, `off` or `no`
/// (case-insensitive, surrounding spaces ignored).
pub(crate) fn enabled_from(value: Option<&str>) -> bool {
    !value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
}

/// [`enabled_from`] for the real environment.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(ENV).ok().as_deref())
}

/// Append an `Examples:` section to a newline-terminated `page`: a blank line,
/// then each example as a `#` comment above its command line.
pub(crate) fn push(page: &mut String, examples: &[Example]) {
    page.push_str("\nExamples:\n");
    for (comment, command) in examples {
        page.push_str(&format!("  # {comment}\n  {command}\n"));
    }
}

/// `body`, then the examples when `show`, then `footer`. The one place a page
/// gets its examples, so the top-level page (footer: its pointer line) and a
/// command page (no footer) are built the same way.
pub(crate) fn page(body: String, examples: &[Example], show: bool, footer: &str) -> String {
    let mut page = body;
    if show {
        push(&mut page, examples);
        if !footer.is_empty() {
            page.push('\n');
        }
    }
    page.push_str(footer);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_hides_the_examples_only_for_the_off_values() {
        for off in ["0", "false", "FALSE", "Off", "no", " 0 "] {
            assert!(!enabled_from(Some(off)), "{off:?}");
        }
        for on in ["1", "true", "on", "yes", "", "anything"] {
            assert!(enabled_from(Some(on)), "{on:?}");
        }
        assert!(enabled_from(None), "unset shows the examples");
    }

    #[test]
    fn page_adds_a_section_and_nothing_else() {
        let ex: &[Example] = &[("Say hi", "relate -c \"RETURN 1\"")];
        // No footer: the examples are a suffix.
        assert_eq!(page("body\n".into(), ex, false, ""), "body\n");
        assert_eq!(
            page("body\n".into(), ex, true, ""),
            "body\n\nExamples:\n  # Say hi\n  relate -c \"RETURN 1\"\n"
        );
        // A footer stays last and is set off by a blank line only when the
        // examples are shown.
        assert_eq!(page("body\n".into(), ex, false, "end\n"), "body\nend\n");
        assert_eq!(
            page("body\n".into(), ex, true, "end\n"),
            "body\n\nExamples:\n  # Say hi\n  relate -c \"RETURN 1\"\n\nend\n"
        );
    }

    #[test]
    fn the_top_level_pair_is_one_or_two_relate_command_lines() {
        assert!((1..=2).contains(&TOP_LEVEL.len()));
        for (comment, command) in TOP_LEVEL {
            assert!(!comment.is_empty());
            assert!(command.starts_with("relate "), "{command}");
        }
    }
}
