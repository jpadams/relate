//! Numbers and errors crossing the JS boundary.

use relate::Error;
use wasm_bindgen::prelude::*;

const SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

pub(crate) fn as_safe(n: u64) -> relate::Result<f64> {
    if n > SAFE_INTEGER {
        Err(Error::storage(
            "OPFS offset exceeds the JavaScript safe integer range",
        ))
    } else {
        Ok(n as f64)
    }
}

pub(crate) fn checked_len(n: f64) -> relate::Result<u64> {
    if !n.is_finite() || n < 0.0 || n > SAFE_INTEGER as f64 {
        Err(Error::storage(
            "OPFS returned a length this engine cannot store",
        ))
    } else {
        Ok(n as u64)
    }
}

pub(crate) fn checked_u32(n: f64, key: &str) -> relate::Result<u32> {
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
        return Err(Error::storage(format!(
            "OPFS {key} is not an integer this engine can store"
        )));
    }
    Ok(n as u32)
}

pub(crate) fn js_u32(value: &JsValue, key: &str) -> relate::Result<u32> {
    let got = js_sys::Reflect::get(value, &JsValue::from_str(key)).map_err(js_storage)?;
    let Some(n) = got.as_f64() else {
        return Err(Error::storage(format!("OPFS prepare is missing {key}")));
    };
    checked_u32(n, key)
}

pub(crate) fn js_storage(err: JsValue) -> Error {
    Error::storage(js_message(&err))
}

pub(crate) fn js_open_err(err: JsValue) -> Error {
    let message = js_message(&err);
    if js_name(&err).as_deref() == Some("RelateLocked") {
        let path = message.trim_start_matches("storage locked:").trim();
        return Error::Locked(path.to_string());
    }
    Error::storage(message)
}

fn js_name(err: &JsValue) -> Option<String> {
    js_sys::Reflect::get(err, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
}

fn js_message(err: &JsValue) -> String {
    if let Some(text) = err.as_string() {
        return text;
    }
    js_sys::Reflect::get(err, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| "OPFS operation failed".to_string())
}
