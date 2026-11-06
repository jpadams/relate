//! The benchmark question set and the correctness gate.
//!
//! One question: the prompt the agent answers with the CLI, the Cypher
//! that seeds the database it answers against, and the expected answer
//! text — the A/B's honesty column is only as good as the gate is exact.

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Question {
    pub(crate) id: String,
    pub(crate) prompt: String,
    #[serde(rename = "setupCypher")]
    pub(crate) setup_cypher: String,
    #[serde(rename = "expectedAnswer")]
    pub(crate) expected_answer: String,
}

/// Load and gate the questions file: every question must carry a
/// single-word `expectedAnswer` the gate can actually match — a
/// question the gate can never mark correct is noise in the honesty
/// column, so it fails loudly here, before any session runs.
pub(crate) fn load(path: &Path) -> Vec<Question> {
    let questions: Vec<Question> =
        serde_json::from_reader(std::fs::File::open(path).expect("the questions file opens"))
            .expect("the questions file is a JSON array of questions");
    for question in &questions {
        assert!(
            !question.expected_answer.is_empty()
                && question.expected_answer.chars().all(char::is_alphanumeric),
            "question {}'s expectedAnswer must be a single alphanumeric word — \
             the correctness gate matches whole words",
            question.id
        );
    }
    assert!(
        !questions.is_empty(),
        "the questions file has no questions — zero runs are not a measurement"
    );
    questions
}

/// A word is a maximal alphanumeric run — the one definition of a word
/// the correctness gate matches answers with (and the load gate
/// requires of every `expectedAnswer`, so a question the gate could
/// never mark correct fails before any session runs).
pub(crate) fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric())
}

/// The correctness gate: the expected answer must appear as a whole
/// word of the final text — never a substring ('6' must not mark '16'
/// correct, nor 'P1' the 'P120' listing). Case is ignored; the word
/// split drops the punctuation a reply wraps answers in.
pub(crate) fn answer_is(final_text: &str, expected: &str) -> bool {
    let expected = expected.to_lowercase();
    words(final_text).any(|token| token.to_lowercase() == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate's whole-word contract: the near-miss numbers and the
    /// prefix listings must fail, the wrapped real answer must pass.
    #[test]
    fn the_gate_matches_whole_words_only() {
        assert!(answer_is("6", "6"));
        assert!(answer_is("There are 6 nodes.", "6"));
        assert!(!answer_is("16", "6"));
        assert!(!answer_is("The answer is 60", "6"));
        assert!(!answer_is("P1 P2 P3", "P120"));
        assert!(answer_is("P1 P2 ... P120", "P120"));
        assert!(answer_is("UPPSALA", "Uppsala"));
    }

    /// The load gate rejects an expectedAnswer the matcher could never
    /// mark correct: multi-word answers fail before any session runs.
    #[test]
    fn the_load_gate_rejects_an_unmatchable_answer() {
        assert_eq!(words("6").count(), 1);
        assert_eq!(words("New York").count(), 2);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("q.json");
        std::fs::write(
            &file,
            serde_json::to_string(&[serde_json::json!({
                "id": "multi-word",
                "prompt": "p",
                "setupCypher": "CREATE ()",
                "expectedAnswer": "New York"
            })])
            .unwrap(),
        )
        .unwrap();
        let result = std::panic::catch_unwind(|| load(&file));
        assert!(
            result.is_err(),
            "a multi-word expectedAnswer must fail the load gate"
        );
    }
}
