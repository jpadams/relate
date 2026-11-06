//! Synchronous OPFS slot pool (ADR-0145).
//!
//! JavaScript allocates the catalog and slot handles before the engine runs.
//! After that, every [`Vfs`] call is a sync access-handle operation. The
//! catalog is the virtual directory: a path maps to a slot id, and directories
//! are prefixes.

mod catalog;
mod js;
mod path;
mod slot;
mod vfs;

pub(crate) use vfs::OpfsVfs;

#[cfg(test)]
mod tests;
