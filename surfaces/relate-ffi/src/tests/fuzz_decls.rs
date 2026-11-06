//! The fuzz harness's `extern "C"` declarations are a hand mirror of
//! `include/relate.h` (an extern declaration against a `#[no_mangle]`
//! symbol is not type-checked across crates, so a header change would
//! otherwise link silently against a stale mirror). This drift guard
//! fails the gate instead: every declaration the mirror carries matches
//! a header prototype''s name and arity exactly, and names a prototype
//! that still exists — so a renamed, re-arithmetied or removed prototype
//! fails loudly. (The mirror deliberately declares only what the harness
//! drives, a subset of the header.)

/// The text with `/* … */` blocks and `// …` lines removed — the header's
/// only non-comment code is declarations, so every `relate_*(` in the
/// stripped text is a prototype, never a doc mention.
fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// `(name, parameter count)` for every `relate_*` prototype: the
/// identifier before a top-level `(`, with commas counted at depth 0
/// through the matching `)`. `(void)` and `()` are zero parameters.
fn prototypes(text: &str) -> Vec<(String, usize)> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '(' {
            let mut j = i;
            while j > 0 && !bytes[j - 1].is_alphanumeric() && bytes[j - 1] != '_' {
                j -= 1;
            }
            let end = j;
            while j > 0 && (bytes[j - 1].is_alphanumeric() || bytes[j - 1] == '_') {
                j -= 1;
            }
            let name: String = bytes[j..end].iter().collect();
            if name.starts_with("relate_") {
                let mut depth = 1;
                let mut commas = 0;
                let mut k = i + 1;
                while k < bytes.len() && depth > 0 {
                    match bytes[k] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        ',' if depth == 1 => commas += 1,
                        _ => {}
                    }
                    k += 1;
                }
                let inner: String = bytes[i + 1..k - 1].iter().collect();
                let trimmed = inner.trim();
                // A trailing comma (the Rust declaration style) already
                // counts the last parameter; the C prototype style does
                // not.
                let params = if trimmed.is_empty() || trimmed == "void" {
                    0
                } else if trimmed.ends_with(',') {
                    commas
                } else {
                    commas + 1
                };
                out.push((name, params));
                i = k;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `(name, parameter count)` for every `pub fn relate_*` declaration
/// in the fuzz crate's mirror (the same matching-paren count, so a
/// multi-line declaration's arity is read whole).
fn declared(text: &str) -> Vec<(String, usize)> {
    prototypes(&text.replace("pub fn ", ""))
}

#[test]
fn fuzz_decls_mirror_the_header_exactly() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let header =
        std::fs::read_to_string(format!("{manifest}/include/relate.h")).expect("the header");
    let decls = std::fs::read_to_string(format!("{manifest}/fuzz/src/lib.rs"))
        .expect("the fuzz crate's declaration mirror");
    // The mirror declares what the harness drives — a subset of the
    // header's prototypes by design — so the guard is subset-with-equality:
    // every declaration must match a header prototype's name and arity
    // exactly, and every name must still exist in the header.
    let header = prototypes(&strip_comments(&header));
    let decls = declared(&decls);
    for (name, params) in &decls {
        assert_eq!(
            header.iter().filter(|(h, _)| h == name).count(),
            1,
            "the fuzz crate declares {name}, which is not exactly one header prototype"
        );
        let h = header
            .iter()
            .find(|(h, _)| h == name)
            .expect("checked above");
        assert_eq!(
            h.1, *params,
            "the fuzz crate's declaration of {name} drifted from include/relate.h"
        );
    }
    assert!(
        !decls.is_empty(),
        "the mirror lost every declaration — the parser or the mirror broke"
    );
}
