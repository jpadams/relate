//! Bound `$__AUTO_*__` values visible to compile-time literal checks.
//!
//! The plan cache compiles the auto-parameterized statement (ADR-0503).
//! Those placeholders stand for the literals they replaced, so a check
//! that rejects a literal must reject the placeholder the same way. The
//! map is installed for one parse or plan and restored on drop, including
//! when that plan compiles a nested statement on the same thread.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::Value;

thread_local! {
    static AUTO_LITERALS: RefCell<Option<Arc<BTreeMap<String, Value>>>> = const { RefCell::new(None) };
}

/// Installs `params` for the guard's lifetime. A map with no `$__AUTO_*__`
/// name installs nothing: a caller's own parameters stay runtime-typed.
pub struct AutoLiteralGuard {
    prev: Option<Arc<BTreeMap<String, Value>>>,
}

impl AutoLiteralGuard {
    pub fn install(params: &BTreeMap<String, Value>) -> Self {
        let next = params
            .keys()
            .any(|name| name.starts_with("__AUTO_"))
            .then(|| Arc::new(params.clone()));
        let prev = AUTO_LITERALS.with(|slot| slot.replace(next));
        Self { prev }
    }
}

impl Drop for AutoLiteralGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        AUTO_LITERALS.with(|slot| *slot.borrow_mut() = prev);
    }
}

/// The literal a plan-cache placeholder stands for, when one is installed.
/// Any other name, including a caller's parameter, is `None`.
pub fn auto_literal(name: &str) -> Option<Value> {
    if !name.starts_with("__AUTO_") {
        return None;
    }
    AUTO_LITERALS.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|map| map.get(name).cloned())
    })
}
