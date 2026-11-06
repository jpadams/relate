//! The ABI's one error channel (ADR-0502 §2), whole: the status enum, the
//! C-string decodes, the shape's renderers (engine failure, boundary
//! failure, caught panic) and the report into the caller's `error_json`
//! out-param. One module because `tabular`, `params` and `value` are all
//! clients of it — a client never imports its siblings' foundations —
//! and a client of the channel imports it from this one site.
//!
//! There is no thread-local error slot, so the channel is async-safe by
//! construction (a call run on a worker thread reports its own error).
//! The shape is a JSON object rendered from the engine's one
//! classification beside the failure's message:
//!
//! ```json
//! {"code":"<Neo4j status code>","gql_status":"<GQLSTATUS>",
//!  "stage":"compile|runtime|internal","retryable":<bool>,
//!  "message":"<message>"}
//! ```

#![allow(non_camel_case_types)]

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::OnceLock;

use relate::{
    client_message, error_info, log_internal_original, Error, ErrorInfo, INTERNAL_ERROR_INFO,
    INTERNAL_MESSAGE,
};

/// A failure already rendered into the one error shape of ABI v2
/// (ADR-0502 §2) — what [`report`] writes into the caller's `error_json`
/// out-param and what the guards (`tabular`) return. Distinct from a
/// plain message string so the two halves cannot be confused: the
/// decoders and builders (an argument decode, a putter's build closure)
/// fail with a plain message their one boundary renders through
/// [`boundary_error_json`]; everything past that point traffics in this
/// type.
pub(crate) struct RenderedError(String);

impl RenderedError {
    /// The rendered shape, for a reader that keeps the type.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The rendered shape as the NUL-terminated C string the out-param
    /// carries — the one conversion, shared by the out-param fill and the
    /// error result. The shape's only free text is the message,
    /// JSON-escaped by the serializer; an interior NUL is impossible, so
    /// construction is the invariant.
    pub(crate) fn into_c_string(self) -> CString {
        CString::new(self.as_str()).expect("the error JSON contains no interior NUL")
    }
}

#[repr(C)]
pub enum relate_status {
    RELATE_OK = 0,
    RELATE_ERROR = 1,
}

/// Whether this boundary's own log of internal-class detail — the
/// original internal message and panic payloads — is on. The embedding
/// host's stderr is the host's channel, so redaction is the default and
/// `RELATE_FFI_LOG=1` opts in for debugging (read once per process).
fn internal_detail_log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("RELATE_FFI_LOG").is_some_and(|v| v == "1"))
}

/// The message a caught panic reports: the fixed internal-class message
/// (ADR-0260, OPLANE_REQ-00340359). A panic payload is engine internals —
/// type names, paths, assertion text — so it never crosses the boundary;
/// with `RELATE_FFI_LOG=1` it is logged library-side (the payload is
/// downcast first: `Box<dyn Any>`'s own Debug prints nothing usable).
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if internal_detail_log_enabled() {
        let detail = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        eprintln!("ffi panic: {detail}");
    }
    INTERNAL_MESSAGE.to_string()
}

/// The one error shape of ABI v2 (ADR-0502 §2) as a typed document: the
/// field order below is the shape's documented key order, and every
/// string field crosses through the JSON serializer — the codes and the
/// stage are constants today, but the document stays valid JSON the day
/// one of them carries a character the format reserves.
#[derive(serde::Serialize)]
struct ErrorShape<'a> {
    code: &'a str,
    gql_status: &'a str,
    stage: &'a str,
    retryable: bool,
    message: &'a str,
}

/// Render the one error shape of ABI v2 (ADR-0502 §2): the engine's
/// classification beside the failure's message, in the documented key
/// order. Serializing a struct of strings and a bool cannot fail. The one
/// construction site of [`RenderedError`] — both renderers below return
/// its value directly, so the type's construction set is exactly the
/// rendering of the shape.
pub(crate) fn error_shape_json(info: &ErrorInfo, message: &str) -> RenderedError {
    RenderedError(
        serde_json::to_string(&ErrorShape {
            code: info.neo4j_code,
            gql_status: info.gql_status,
            stage: info.stage.as_str(),
            retryable: info.retryable,
            message,
        })
        .expect("serializing the error shape cannot fail"),
    )
}

/// Render the error shape for an engine failure: the engine's one
/// classification ([`error_info`]) beside the message its
/// [`ErrorInfo::client_message`] allows across — the ADR-0260
/// message-exposure policy, single-sourced in `relate-error` beside
/// the classification: the caller-facing classes' Display text, the fixed
/// [`INTERNAL_MESSAGE`] for the internal class (engine-internal, IO,
/// storage and locked messages can carry file paths and internals). With
/// `RELATE_FFI_LOG=1` the internal class's original is logged
/// library-side for debugging.
pub(crate) fn engine_error_json(e: &Error) -> RenderedError {
    let info = error_info(e);
    if internal_detail_log_enabled() {
        log_internal_original(e, "ffi");
    }
    error_shape_json(&info, &client_message(e))
}

/// Render the error shape for an FFI-boundary failure — a null or poisoned
/// handle, a caught panic, an argument the engine never saw: the
/// internal-class row of the engine's table (ADR-0260 — not the statement's
/// fault, never retryable), the row `relate-error` publishes as
/// [`INTERNAL_ERROR_INFO`] for exactly the boundaries that fail without an
/// engine error to classify, beside the boundary's own message.
pub(crate) fn boundary_error_json(message: &str) -> RenderedError {
    error_shape_json(&INTERNAL_ERROR_INFO, message)
}

/// Read a C string argument, mapping null/invalid-UTF-8 to an error message.
///
/// # Safety
/// `p` must be null or point to a valid NUL-terminated C string.
pub(crate) unsafe fn read_cstr(p: *const c_char) -> Result<String, String> {
    if p.is_null() {
        return Err("null pointer".to_string());
    }
    // SAFETY: `p` is non-null and valid per this function's contract.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| "invalid utf-8".to_string())
}

/// Read a NUL-terminated C string, refusing when it runs past `cap` —
/// one walk: the scan that finds the limit is the decode, so an
/// over-cap name is refused before its bytes are copied and the length
/// is never computed twice (OPLANE_REQ-00340357). The bounded half of
/// the decode policy, beside [`read_cstr`]'s trust-contract half: the
/// unbounded read serves arguments the contract already bounds, this one
/// serves the params builder's capped string arguments.
///
/// # Safety
/// `s` must be non-null and point to a NUL-terminated string (the ABI's
/// trusted-length contract); the scan reads at most `cap + 1` bytes of it.
pub(crate) unsafe fn read_cstr_bounded(
    s: *const c_char,
    cap: usize,
    what: &str,
) -> Result<String, String> {
    if s.is_null() {
        return Err("null pointer".to_string());
    }
    // SAFETY: non-null (checked above) and NUL-terminated per this
    // function's contract.
    unsafe {
        for i in 0..=cap {
            if *s.add(i) == 0 {
                let bytes = std::slice::from_raw_parts(s.cast::<u8>(), i);
                let text = std::str::from_utf8(bytes).map_err(|_| "invalid utf-8".to_string())?;
                return Ok(text.to_string());
            }
        }
    }
    Err(format!("{what} exceeds the {cap}-byte limit"))
}

/// Report a guarded call's outcome on the v2 error channel (ADR-0502 §2):
/// success passes through as `RELATE_OK`; a failure fills the caller's
/// trailing `char **error_json` out-param with the rendered error shape —
/// a NULL out-param opts out of the detail — and returns
/// `RELATE_ERROR`. The one out-param fill: every failure that reaches
/// a caller passes through here.
///
/// # Safety
/// `error_json` must be null or a writable out-param per the C header.
pub(crate) unsafe fn report(
    result: Result<(), RenderedError>,
    error_json: *mut *mut c_char,
) -> relate_status {
    match result {
        Ok(()) => relate_status::RELATE_OK,
        Err(json) => {
            if !error_json.is_null() {
                // SAFETY: `error_json` is non-null (checked above) and
                // writable per the caller contract.
                unsafe { *error_json = json.into_c_string().into_raw() };
            }
            relate_status::RELATE_ERROR
        }
    }
}
