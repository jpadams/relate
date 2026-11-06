//! The closed contract's test-only tarball builder (ADR-0431 §2): the
//! encoder the unpacker's own contract tests and the install flow's
//! fixture-index driver pack fixture artifacts with — one builder beside
//! the one reader (`unpack.rs`), so a fixture the tests build can never
//! drift from the contract the reader enforces. Gated beside its
//! consumers on `extension-install` (the unpacker's own feature); the
//! e2e suites' twin lives in `tests/support/extensions.rs` (a test
//! target cannot reach this `cfg(test)` module, and the crate cannot
//! reach the test dir).

use super::unpack::{round_up_to_block, set_ustar_checksum, HEADER_BYTES};

/// Build one ustar header for a regular member, checksum stamped by the
/// canonical helper the reader verifies by — the encoder never carries a
/// second checksum of its own.
fn header(name: &str, size: u64, mode: u32, typeflag: u8) -> Vec<u8> {
    let mut block = vec![0u8; HEADER_BYTES];
    block[..name.len()].copy_from_slice(name.as_bytes());
    block[100..108].copy_from_slice(format!("{mode:07o}\0").as_bytes());
    block[108..116].copy_from_slice(b"0000000\0"); // uid
    block[116..124].copy_from_slice(b"0000000\0"); // gid
    block[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    block[136..148].copy_from_slice(b"00000000000\0"); // mtime
    block[156] = typeflag;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    set_ustar_checksum(&mut block);
    block
}

/// A gzip'd tarball carrying the given (name, mode, payload) regular
/// members — the closed contract's archive, built with the same
/// in-tree pieces the unpacker reads.
pub(crate) fn tarball(members: &[(&str, u32, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, mode, payload) in members {
        tar.extend_from_slice(&header(name, payload.len() as u64, *mode, b'0'));
        tar.extend_from_slice(payload);
        let pad = round_up_to_block(payload.len()) - payload.len();
        tar.extend(std::iter::repeat_n(0u8, pad));
    }
    tar.extend([0u8; HEADER_BYTES * 2]);
    gzip(&tar)
}

/// Gzip raw tar bytes: the re-encode tail a test that splices a tar's
/// members needs (junk rides inside the tar, after the end marker, not
/// after the gzip stream — a gzip decoder stops at its member's end and
/// would never see the latter).
pub(crate) fn gzip(tar: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(tar).unwrap();
    gz.finish().unwrap()
}
