//! The headless session's event stream, typed the way `Question` is:
//! one enum tagged on the event `type`, the message content a tagged
//! enum of the block shapes the metrics read, a tool result's content a
//! str-or-parts untagged enum. A transcript line that breaks the
//! contract fails loudly at the parse in the harness — never as a
//! silently-zeroed metric.

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    User {
        message: Message,
    },
    Assistant {
        message: Message,
    },
    /// The session's final result: its text is the answer the
    /// correctness gate reads (a session may end with tool use, not a
    /// text block), and its `usage` object is the canonical token
    /// totals — always present in this claude's stream shape (`cost` is
    /// null on the result event, verified), so a stream that omits it
    /// fails loudly at the parse, never as a silently-zeroed metric.
    Result {
        result: Option<String>,
        usage: Usage,
    },
    /// Every other event kind the stream emits (`system`, stream frames,
    /// …) — the harness measures only the three shapes above.
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(crate) struct Message {
    /// The assistant's text and tool_use blocks (the harness reads the
    /// text; tool use is invisible here). A user event's content carries
    /// the tool results — the metric.
    pub(crate) content: Option<Vec<Block>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Block {
    /// A tool result's content is what the agent read back — the metric.
    ToolResult {
        content: ToolContent,
    },
    Text {
        text: String,
    },
    /// Other block kinds (thinking, tool use) are neither tool output
    /// nor answer text: both the byte metric and the answer gate
    /// intentionally skip them.
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolContent {
    Str(String),
    Parts(Vec<Part>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Part {
    Text {
        text: String,
    },
    /// Any other part shape inside a tool result's content — the byte
    /// metric knows how to read only text, so this is a contract break
    /// that fails loudly in the measure, never a silently-zeroed count.
    #[serde(other)]
    Unknown,
}

/// The session's token totals — the result event's canonical numbers,
/// non-optional fields: the stream always carries them, and one that
/// does not fails loudly at the parse instead of zeroing the metric.
#[derive(Deserialize)]
pub(crate) struct Usage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}

/// The last text block of one message's content — the answer text the
/// correctness gate reads (a session may end with tool use, not a text
/// block). `measure` reads it and the tests pin it: one selection
/// policy beside the model it reads.
pub(crate) fn last_text_block(content: Option<&[Block]>) -> Option<&str> {
    content
        .into_iter()
        .flatten()
        .filter_map(|block| match block {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .next_back()
}

/// Parse one transcript line — the loud parse: a line that fails names
/// itself, because the session transcript is a machine contract, not
/// untrusted input.
pub(crate) fn parse_line(line: &str) -> Event {
    serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("unparsable transcript line ({e}): {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The typed stream parses the three measured shapes and tolerates
    /// every other event kind.
    #[test]
    fn the_stream_parses_the_measured_shapes_and_tolerates_the_rest() {
        assert!(matches!(
            parse_line(r#"{"type":"system","subtype":"init"}"#),
            Event::Unknown
        ));
        let user = parse_line(
            r#"{"type":"user","message":{"content":[
                {"type":"tool_result","content":"rows here"}]}}"#,
        );
        let Event::User { message } = user else {
            panic!("a user event parsed as one");
        };
        assert_eq!(
            message.content.as_deref().map(|blocks| blocks.len()),
            Some(1)
        );
        let assistant = parse_line(
            r#"{"type":"assistant","message":{"content":
                [{"type":"text","text":"hi"},{"type":"thinking"}]}}"#,
        );
        let Event::Assistant { message } = assistant else {
            panic!("an assistant event parsed as one");
        };
        assert_eq!(
            last_text_block(message.content.as_deref()),
            Some("hi"),
            "the last text block is the answer text"
        );
        let result = parse_line(
            r#"{"type":"result","result":"42","usage":
                {"input_tokens":100,"output_tokens":5}}"#,
        );
        let Event::Result { result, usage } = result else {
            panic!("a result event parsed as one");
        };
        assert_eq!(result.as_deref(), Some("42"));
        assert_eq!((usage.input_tokens, usage.output_tokens), (100, 5));
    }

    /// The canonical totals are a required field: a result event
    /// without usage fails loudly at the parse, never as a zero metric.
    #[test]
    fn a_result_event_without_usage_fails_loudly() {
        let result = std::panic::catch_unwind(|| parse_line(r#"{"type":"result","result":"42"}"#));
        assert!(result.is_err(), "a result event must carry its usage");
    }

    /// A part without a `type` tag, or a shape outside the contract,
    /// fails loudly at the parse — never a silently-zeroed metric.
    #[test]
    fn an_unparsable_line_names_itself() {
        let result = std::panic::catch_unwind(|| parse_line("not json"));
        assert!(result.is_err(), "a broken line must fail loudly");
    }
}
