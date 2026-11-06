#!/usr/bin/env bash
# mcp-client.sh — the bash half of the MCP mount's client kit (ADR-0447).
#
# Source it from a bash extension and call the host's tools:
#
#     source mcp-client.sh
#     mcp_init my-extension 0.1.0
#     created=$(mcp_call update_schema '{"statement":"CREATE INDEX …"}')
#     mcp_call write_cypher '{"statement":"…","parameters":{"rows":[…]}}' >/dev/null
#     mcp_call report_output '{"text":"imported …"}' >/dev/null
#
# A package ships the manifest plus ONE executable, so an installed package
# cannot source a second file: `so-to-graph.sh` inlines this kit verbatim
# between its markers, and scripts/check-extension-examples.sh pins the two
# copies together.
# The MCP revision this kit speaks — the wire kit's own constant
# (`relate_wire::mcp::MCP_PROTOCOL_VERSION`). The host's flag values are
# what it acts on; the revision is the client's half of the handshake, and
# `mcp_init` fails a host that answers another one instead of downgrading.
MCP_PROTOCOL_VERSION=${MCP_PROTOCOL_VERSION:-2025-06-18}

# The host's request envelope, once `mcp_init` read it.
MCP_ENVELOPE=
# The last correlation id sent; the handshake takes 1.
MCP_ID=0
# The initialize result, once `mcp_init` read it.
MCP_INIT_RESULT=
# The host's request pipe (fd 3, set by `mcp_init`): requests go here, so a
# `$(mcp_call …)` capture takes the tool result and not the request.
MCP_FD=

mcp_fail() {
    printf 'mcp: %s\n' "$*" >&2
    exit 1
}

# The raw JSON token of flag NAME from the envelope (`"rust"`, `2`,
# `false`, `["rust","cypher"]`). Fails when the manifest and the child
# disagree about the flag table.
_mcp_flag_value() {
    local rest
    case $MCP_ENVELOPE in
        *"\"$1\":"*) rest=${MCP_ENVELOPE#*\"$1\":} ;;
        *) mcp_fail "the request envelope carries no '$1' flag" ;;
    esac
    case $rest in
        \[*)
            rest=${rest#\[}
            printf '[%s' "${rest%%\]*}"
            ;;
        *)
            # A scalar ends at the next `,` — or at the envelope's closing
            # `}` when the flag is the last one. The two cuts stay separate
            # because `}` inside a bracket expression ends the expansion.
            rest=${rest%%,*}
            printf '%s' "${rest%%\}*}"
            ;;
    esac
}

# _mcp_decode_string TOKEN — the value of a JSON string literal, its quotes
# included, with `json_string`'s escapes undone: the one decoder both the
# envelope's flag readers and the tool results use. Fails on a token that is
# not a string.
_mcp_decode_string() {
    local rest=$1 value= char
    case $rest in
        \"*) rest=${rest#\"} ;;
        *) mcp_fail "not a JSON string: $rest" ;;
    esac
    while [ -n "$rest" ]; do
        char=${rest:0:1}
        rest=${rest:1}
        # A backslash escapes the next character: the four JSON shortcuts,
        # and the quote/backslash themselves.
        if [ "$char" = '\' ]; then
            char=${rest:0:1}
            rest=${rest:1}
            case $char in
                n) value+=$'\n' ;;
                t) value+=$'\t' ;;
                r) value+=$'\r' ;;
                *) value+=$char ;;
            esac
            continue
        fi
        [ "$char" = '"' ] && break
        value+=$char
    done
    printf '%s' "$value"
}

# mcp_flag_string NAME — a string flag's value, unquoted and unescaped.
mcp_flag_string() {
    local value
    value=$(_mcp_flag_value "$1")
    case $value in
        \"*) _mcp_decode_string "$value" ;;
        *) mcp_fail "flag '$1' is not a string: $value" ;;
    esac
}

# mcp_flag_int NAME — an integer flag's value.
mcp_flag_int() {
    local value
    value=$(_mcp_flag_value "$1")
    case $value in
        '' | *[!0-9]*) mcp_fail "flag '$1' is not an integer: $value" ;;
    esac
    printf '%s' "$value"
}

# mcp_flag_bool NAME — a boolean flag's value: `true` or `false`.
mcp_flag_bool() {
    local value
    value=$(_mcp_flag_value "$1")
    case $value in
        true | false) printf '%s' "$value" ;;
        *) mcp_fail "flag '$1' is not a boolean: $value" ;;
    esac
}

# mcp_flag_list NAME — a repeatable flag's values, one per line (nothing
# when the list is empty).
mcp_flag_list() {
    local value
    value=$(_mcp_flag_value "$1")
    case $value in
        \[*) ;;
        *) mcp_fail "flag '$1' is not a list: $value" ;;
    esac
    value=${value#\[}
    case $value in
        \"*\")
            value=${value#\"}
            value=${value%\"}
            ;;
    esac
    [ -z "$value" ] && return 0
    value=${value//\"/}
    printf '%s\n' "${value//,/$'\n'}"
}

# mcp_init [NAME] [VERSION] — read the host's envelope, complete the
# initialize handshake, and tell the host the client is ready.
mcp_init() {
    local name=${1:-extension} version=${2:-0.1.0}
    # Duplicate the host's pipe before anything can be captured: stdout is
    # the MCP request channel from here on.
    exec 3>&1
    MCP_FD=3
    IFS= read -r MCP_ENVELOPE ||
        mcp_fail "the host wrote no request envelope"
    MCP_ID=1
    printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"%s","capabilities":{},"clientInfo":{"name":"%s","version":"%s"}}}\n' \
        "$MCP_PROTOCOL_VERSION" "$name" "$version" >&3
    IFS= read -r MCP_INIT_RESULT ||
        mcp_fail "the host closed the session before the initialize response"
    case $MCP_INIT_RESULT in
        *'"error":'*) mcp_fail "initialize refused: $MCP_INIT_RESULT" ;;
        *'"result":'*) ;;
        *) mcp_fail "initialize answered no result: $MCP_INIT_RESULT" ;;
    esac
    # MCP's rule for a revision the server does not serve: it answers with
    # the one it does, and a client that cannot speak that one must fail the
    # handshake — a session the two sides disagree about is worse than none.
    case $MCP_INIT_RESULT in
        *'"protocolVersion":"'"$MCP_PROTOCOL_VERSION"'"'*) ;;
        *)
            mcp_fail "initialize answered a protocolVersion other than '$MCP_PROTOCOL_VERSION': $MCP_INIT_RESULT"
            ;;
    esac
    # The standard follow-up; the host reads notifications and ignores them.
    printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n' >&3
}

# mcp_call TOOL ARGS_JSON — one `tools/call`, its arguments the JSON
# object ARGS_JSON. Prints the tool's own JSON result (the MCP text
# content, unescaped); a JSON-RPC error or an unreadable response exits
# the child through `mcp_fail`.
mcp_call() {
    local tool=$1 arguments=${2:-'{}'} response text
    case $arguments in
        \{*) ;;
        *) mcp_fail "mcp_call '$tool': the arguments must be a JSON object" ;;
    esac
    [ -n "$MCP_FD" ] || mcp_fail "mcp_call '$tool' before mcp_init"
    MCP_ID=$((MCP_ID + 1))
    printf '{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}\n' \
        "$MCP_ID" "$tool" "$arguments" >&3
    IFS= read -r response ||
        mcp_fail "the host closed the session before answering '$tool'"
    case $response in
        *'"error":'*) mcp_fail "'$tool' refused: $response" ;;
        *'"content":'*) ;;
        *) mcp_fail "'$tool' answered no tool content: $response" ;;
    esac
    case $response in
        *'"text":"'*) ;;
        *) mcp_fail "'$tool' answered no text content: $response" ;;
    esac
    text=${response#*\"text\":\"}
    case $text in
        *'","type":"'*) ;;
        *) mcp_fail "'$tool' answered an unterminated text content: $response" ;;
    esac
    text=${text%%\",\"type\":*}
    # The tool result is JSON carried as a JSON string: `\"` is the string
    # encoding's quote, and `%b` undoes the rest (`\n`, `\\`, ...).
    text=${text//\\\"/\"}
    printf '%b' "$text"
}

# mcp_number JSON NAME — the number the JSON object JSON carries under
# NAME (the counters, the read counts). Empty when absent.
mcp_number() {
    local rest=$1
    case $1 in
        *"\"$2\":"*) rest=${1#*\"$2\":} ;;
        *) return 0 ;;
    esac
    rest=${rest%%,*}
    printf '%s' "${rest%%\}*}"
}

# mcp_string JSON NAME — the string the JSON object JSON carries under NAME,
# with the escapes `json_string` writes undone. Empty when absent. The value
# ends at the first quote no backslash escapes, so a quote inside it decodes
# rather than truncating the read.
mcp_string() {
    local rest=$1
    case $rest in
        *"\"$2\":\""*) rest=${rest#*\"$2\":\"} ;;
        *) return 0 ;;
    esac
    _mcp_decode_string "\"$rest\""
}

# json_string VALUE — VALUE as a JSON string literal, quotes included: the
# argument objects this kit sends are JSON text, so a value from the
# envelope (a flag) is encoded, never spliced in raw.
json_string() {
    local value=$1
    value=${value//\\/\\\\}
    value=${value//\"/\\\"}
    value=${value//$'\n'/\\n}
    value=${value//$'\r'/\\r}
    value=${value//$'\t'/\\t}
    printf '"%s"' "$value"
}
