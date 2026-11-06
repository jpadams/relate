//! The crate's usage(2)/runtime(1) exit split (ADR-0430 §5), stated once.
//! A **usage error** is the invocation being malformed before anything
//! ran — a refused value shape, a conflicting flag pair — and exits 2.
//! A **runtime failure** is work that was invited and failed — a fetch,
//! a clone, an open — and exits 1. Each typed error carrier
//! ([`crate::load::LoadError`], the extensions tree's `InstallError`)
//! projects onto this class and adds only its own message prefix and
//! renderer, so the code mapping can never drift between flows. Ungated:
//! `--load` resolves in every build, so the class it carries must too.

/// One error's exit class: [`ExitClass::Usage`] exits 2,
/// [`ExitClass::Runtime`] exits 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitClass {
    /// The invocation was malformed before anything ran (exit 2,
    /// ADR-0430 §5's split).
    Usage,
    /// The work was invited and failed (exit 1).
    Runtime,
}

impl ExitClass {
    /// The process exit code this class carries — the one statement of
    /// the split; no call site decides a code.
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Usage => 2,
            Self::Runtime => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_exits_two_runtime_exits_one() {
        assert_eq!(ExitClass::Usage.code(), 2);
        assert_eq!(ExitClass::Runtime.code(), 1);
    }
}
