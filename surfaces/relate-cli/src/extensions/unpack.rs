//! The closed unpack contract (ADR-0431 §2): the distribution artifact is a
//! gzip'd tarball holding exactly the package directory — `extension.json`
//! and the executable, regular files only. The member contract is closed
//! and enforced here: no symlinks, no hardlinks, no device nodes, no
//! directories, no pax/gnu extension members, no paths that escape the
//! package directory, no duplicate members — each is a loud refusal
//! (ADR-0429 §2's escaping-executable rule extended to every member),
//! never a skipped entry. The executable lands with its exec bit from the
//! member's mode field.
//!
//! Decompression is `flate2` with the pure-Rust `miniz_oxide` backend —
//! already a workspace dependency (ADR-0385) — and the ustar reader is
//! this in-tree reader for this closed contract (ADR-0431 rejected the
//! `tar` crate: a general reader accepts the members the contract
//! forbids, and the refusal list is the security property).
//!
//! What the unpacker does NOT check is member accounting — that the
//! files unpacked are exactly the set the manifest accounts for. The
//! manifest is inside the tarball, so accounting runs one step later, at
//! the install's staged-package check ([`super::install`]).

use std::path::Path;

use relate_wire::read_cap::{read_capped, CappedRead};

use super::install::error::InstallError;

/// The tar header block size, in bytes (ustar). `pub(crate)` for the
/// test-only tarball builder (`testtar`), which packs fixture archives
/// against the same block fact the reader parses by.
pub(crate) const HEADER_BYTES: usize = 512;

/// The closed member cap (ADR-0431 §2's "exactly the package directory"):
/// a tarball with more members than this refuses before writing them,
/// long before the manifest accounting runs — an unpack cap in the
/// ADR-0377 spirit, a named bound on what an untrusted artifact can make
/// the installer write.
const MAX_MEMBERS: usize = 16;

/// The total unpacked-byte cap: the download is capped at 512 MiB
/// (the shared HTTP client), and the decompressed stream is capped at
/// the same bound **at the read** — [`unpack`] reads the gzip stream
/// through [`read_capped`](relate_wire::read_cap::read_capped), so
/// the decompression allocates at most this bound while the data still
/// arrives and a compressed bomb refuses at the read, before any
/// member parses — never at the filesystem.
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// Unpack `gzip_bytes` (the whole downloaded tarball) into `target` under
/// the closed contract, returning the member names in archive order.
/// `target` must exist and be empty — the installer stages into a fresh
/// sibling directory and renames it into place (ADR-0431 §4 step 4), so
/// a partial unpack never lands at the package path.
pub(crate) fn unpack(gzip_bytes: &[u8], target: &Path) -> Result<Vec<String>, InstallError> {
    // The decompression itself is the allocation site: the stream reads
    // through the one bounded read, so an archive that would unpack past
    // the cap costs one byte past it instead of the stream it carries —
    // the cap is enforced while the data still arrives, never after the
    // whole stream sits in memory.
    let archive = flate2::read::GzDecoder::new(gzip_bytes);
    let tar = match read_capped(archive, MAX_UNPACKED_BYTES) {
        Ok(CappedRead::Complete(tar)) => tar,
        Ok(CappedRead::Exceeded) => {
            return Err(InstallError::Runtime(format!(
                "the tarball decompresses past the {MAX_UNPACKED_BYTES}-byte cap (a compressed \
                 bomb refuses here, not at the filesystem)"
            )));
        }
        // The archive-level gzip refusal names the archive, not a member.
        Err(e) => {
            return Err(InstallError::Runtime(format!(
                "the tarball is not valid gzip: {e}"
            )));
        }
    };
    unpack_ustar(&tar, target)
}

/// The in-tree ustar reader: 512-byte header blocks, each member's
/// payload padded to a block boundary, the end marker an all-zero block
/// with nothing but zero padding after it. Everything outside the
/// contract — a non-regular typeflag, an escaping or duplicate name, a
/// bad header checksum, a short payload, trailing data after the end
/// block — is a typed refusal.
fn unpack_ustar(tar: &[u8], target: &Path) -> Result<Vec<String>, InstallError> {
    let mut members = Vec::new();
    let mut offset = 0usize;
    while offset + HEADER_BYTES <= tar.len() {
        let header = &tar[offset..offset + HEADER_BYTES];
        offset += HEADER_BYTES;
        // The end: the first all-zero block; only zero bytes may follow
        // (checked after the loop — trailing non-zero data refuses).
        if header.iter().all(|b| *b == 0) {
            return finish(tar, offset, members);
        }
        let member = parse_header(header)?;
        // The attacker-controlled size is bounded in u64 before any
        // usize narrowing: a declared size past the bytes the capped
        // stream actually holds refuses here, and the narrowing below
        // is total — no offset arithmetic can overflow on a hostile
        // declared size.
        let remaining = tar.len() - offset;
        if member.size > remaining as u64 {
            return Err(InstallError::Runtime(format!(
                "the tarball ends inside member '{}' ({} bytes declared, {remaining} left)",
                member.name, member.size
            )));
        }
        let size = member.size as usize;
        let payload = &tar[offset..offset + size];
        offset += round_up_to_block(size);
        if members.len() >= MAX_MEMBERS {
            return Err(InstallError::Runtime(format!(
                "the tarball carries more than {MAX_MEMBERS} members (the package contract is \
                 extension.json plus the executable)"
            )));
        }
        // The members list itself is the duplicate table — one list, the
        // archive order, and the duplicate refusal that consults it.
        if members.contains(&member.name) {
            return Err(InstallError::Runtime(format!(
                "the tarball carries a duplicate member '{}'",
                member.name
            )));
        }
        let path = target.join(&member.name);
        // The escaping rule, one canonical helper: the extensions root's
        // lexical package-dir containment (ADR-0429 §2) extended to every
        // member (ADR-0431 §2) — an absolute, rooted, prefixed, or
        // parent-hopping name refuses here.
        if crate::extensions::escapes_package_dir(Path::new(&member.name)) {
            return Err(InstallError::Runtime(format!(
                "the tarball member '{}' escapes the package directory",
                member.name
            )));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| member_refusal(&member.name, &e.to_string()))?;
        }
        std::fs::write(&path, payload).map_err(|e| member_refusal(&member.name, &e.to_string()))?;
        apply_mode(&path, member.mode).map_err(|e| member_refusal(&member.name, &e.to_string()))?;
        members.push(member.name);
    }
    Err(InstallError::Runtime(
        "the tarball has no end-of-archive marker (it is truncated)".to_string(),
    ))
}

/// The loop's tail after the end marker: nothing but zero padding may
/// follow it — anything else is data the contract does not account for.
fn finish(tar: &[u8], offset: usize, members: Vec<String>) -> Result<Vec<String>, InstallError> {
    if tar[offset..].iter().any(|b| *b != 0) {
        return Err(InstallError::Runtime(
            "the tarball carries data after its end-of-archive marker".to_string(),
        ));
    }
    Ok(members)
}

/// A parsed ustar header, already reduced to the closed contract's three
/// decisions: the name, the regular-file size, and the mode the unpacked
/// file takes.
struct ParsedHeader {
    name: String,
    size: u64,
    mode: u32,
}

/// Parse and validate one 512-byte ustar header against the closed
/// contract. The typeflag must be a regular file (`'0'` or NUL); the
/// checksum must match (computed with the checksum field read as
/// spaces, [`ustar_checksum`]); pax/gnu extension members are refused —
/// a general reader's acceptance of them is exactly what the contract
/// forbids (ADR-0431's rejection of the `tar` crate).
fn parse_header(header: &[u8]) -> Result<ParsedHeader, InstallError> {
    let name_field = member_name(&header[0..100])?;
    let prefix_field = member_name(&header[345..500])?;
    let size = parse_octal(&header[124..136])?;
    let mode = parse_mode(&header[100..108])?;
    let typeflag = header[156];
    let name = if prefix_field.is_empty() {
        name_field
    } else {
        format!("{prefix_field}/{name_field}")
    };
    if name.is_empty() {
        return Err(InstallError::Runtime(
            "the tarball carries a member with an empty name".to_string(),
        ));
    }
    // The closed member contract: regular files only. Directory entries,
    // symlinks, hardlinks, devices, fifos, and every extension member
    // (pax 'x'/'g', GNU 'L'/'K', …) refuse with the member named.
    if !matches!(typeflag, b'0' | 0) {
        return Err(InstallError::Runtime(format!(
            "the tarball member '{}' is not a regular file (type flag '{}')",
            name, typeflag as char
        )));
    }
    // The header checksum: the stored sum with the checksum field read as
    // spaces must match the stored value — a corrupted or hand-crafted
    // header refuses here, before its size or name is trusted further.
    let stored = parse_octal(&header[148..156])?;
    let actual = ustar_checksum(header);
    if actual != stored {
        return Err(InstallError::Runtime(format!(
            "the tarball member '{name}' has a bad header checksum (stored {stored}, computed {actual})"
        )));
    }
    Ok(ParsedHeader { name, size, mode })
}

/// The canonical ustar header checksum: the byte sum of the whole
/// 512-byte block with the checksum field itself (bytes 148..156) read
/// as spaces — the one computation the reader verifies by
/// ([`parse_header`]), the fixture encoder stamps
/// ([`set_ustar_checksum`], `testtar`), and the splice test helper
/// restamps after a patch. The e2e layer's twin lives in
/// `tests/support/extensions.rs` (a test target cannot reach the crate).
fn ustar_checksum(block: &[u8]) -> u64 {
    block
        .iter()
        .enumerate()
        .map(|(i, b)| u64::from(if (148..156).contains(&i) { 0x20 } else { *b }))
        .sum()
}

/// Stamp a header block's checksum field from [`ustar_checksum`]: the
/// producer's half of the one canonical rule — the fixture encoder
/// (`testtar`) and the splice test helper write what the reader
/// verifies, never a second hand-rolled sum. Test-only consumers, so
/// the stamp rides the test cfg they do.
#[cfg(test)]
pub(crate) fn set_ustar_checksum(block: &mut [u8]) {
    let sum = ustar_checksum(block);
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

/// A member's on-disk failure: names the member and the cause.
fn member_refusal(name: &str, cause: &str) -> InstallError {
    InstallError::Runtime(format!(
        "unpacking the tarball member '{name}' failed: {cause}"
    ))
}

/// A NUL- or space-terminated header field's bytes — real `tar(1)`
/// terminates its fields with either byte (the ustar spec allows both),
/// so the field ends at the first of them. The shared trim: the name
/// decode and the octal parse both read through it.
fn trimmed_field(field: &[u8]) -> &[u8] {
    let end = field
        .iter()
        .position(|b| *b == 0 || *b == b' ')
        .unwrap_or(field.len());
    &field[..end]
}

/// A header's name (or prefix) field as a member name: the trimmed
/// bytes decoded strictly — the closed contract's member names are
/// UTF-8, and a name that is not refuses with the pinned text. There is
/// no lossy fabrication of an untrusted name: `from_utf8_lossy` would
/// hand the escaping and duplicate checks a name the archive never
/// carried.
fn member_name(field: &[u8]) -> Result<String, InstallError> {
    let bytes = trimmed_field(field);
    std::str::from_utf8(bytes)
        .map(|name| name.to_string())
        .map_err(|_| {
            InstallError::Runtime(
                "the tarball carries a member whose name is not valid UTF-8".to_string(),
            )
        })
}

/// An octal header field (leading spaces or NULs tolerated, GNU base-256
/// refused — the closed contract's tarballs are octal).
fn parse_octal(field: &[u8]) -> Result<u64, InstallError> {
    // An octal field is ASCII by definition: bytes that are not even
    // UTF-8 refuse without echoing their value back.
    let Ok(text) = std::str::from_utf8(trimmed_field(field)) else {
        return Err(InstallError::Runtime(
            "the tarball carries a non-octal header field (a field that is not valid UTF-8)"
                .to_string(),
        ));
    };
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| {
        InstallError::Runtime(format!(
            "the tarball carries a non-octal header field ('{text}')"
        ))
    })
}

/// The member's mode field as the `u32` the landing policy takes. The
/// conversion is explicit and fail-closed rather than a bare `as`, as the
/// header is attacker-controlled — but note the guard is UNREACHABLE
/// today: the field is always the 8-byte header slice, whose largest
/// octal value (0o77777777 = 16,777,215) always fits `u32`. It exists as
/// the guard for a future widening of the field or the octal parser; a
/// widening that made it live would need its own test then.
fn parse_mode(field: &[u8]) -> Result<u32, InstallError> {
    let mode = parse_octal(field)?;
    u32::try_from(mode).map_err(|_| {
        InstallError::Runtime(format!(
            "the tarball carries a member mode field outside the 32-bit range ({mode})"
        ))
    })
}

/// The member count is padded to the block size after the payload.
/// `pub(crate)` for the test-only tarball builder (`testtar`), which
/// pads with the same rule the reader skips by.
pub(crate) fn round_up_to_block(size: usize) -> usize {
    size.div_ceil(HEADER_BYTES) * HEADER_BYTES
}

/// The unpacked file's permissions from the member's mode field, masked to
/// the rwx bits — setuid/setgid/sticky never ride from an archive (the
/// executable's exec bit is the one bit the contract cares about,
/// ADR-0431 §2). The failure is the member's refusal, not a silent
/// permission drift: a chmod that fails propagates to the caller, which
/// words it through [`member_refusal`]. `pub(crate)` so the `--from`
/// install copies its executable under the same one masking policy.
#[cfg(unix)]
pub(crate) fn apply_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))
}

/// Non-unix builds ignore the mode field (there is no exec bit to land).
#[cfg(not(unix))]
pub(crate) fn apply_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::extensions::testtar::{gzip, tarball};

    /// Unpack `bytes` into a fresh staging dir, returning the run's error
    /// or member names — the contract tests' one entry point.
    fn unpack_into(bytes: &[u8]) -> Result<Vec<String>, InstallError> {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("stage");
        std::fs::create_dir(&staging).unwrap();
        unpack(bytes, &staging)
    }

    #[test]
    fn a_contract_tarball_roundtrips_with_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("stage");
        std::fs::create_dir(&staging).unwrap();
        let manifest = br#"{"kind":"command"}"#;
        let script = b"#!/bin/sh\n";
        let bytes = tarball(&[
            ("extension.json", 0o644, manifest.as_slice()),
            ("greet.sh", 0o755, script.as_slice()),
        ]);
        let members = unpack(&bytes, &staging).unwrap();
        assert_eq!(
            members.iter().map(String::as_str).collect::<Vec<_>>(),
            ["extension.json", "greet.sh"]
        );
        assert_eq!(
            std::fs::read(staging.join("extension.json")).unwrap(),
            manifest
        );
        assert_eq!(std::fs::read(staging.join("greet.sh")).unwrap(), script);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(staging.join("greet.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o755,
                "the exec bit lands from the member mode"
            );
            let mode = std::fs::metadata(staging.join("extension.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644);
        }
    }

    #[test]
    fn a_nested_executable_path_unpacks_under_its_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("stage");
        std::fs::create_dir(&staging).unwrap();
        let bytes = tarball(&[("bin/greet", 0o755, b"run\n")]);
        let members = unpack(&bytes, &staging).unwrap();
        assert_eq!(members[0], "bin/greet");
        assert!(staging.join("bin/greet").is_file());
    }

    #[test]
    fn each_contract_violation_refuses_naming_the_member() {
        let tmp = tempfile::tempdir().unwrap();
        for (i, (case, bytes, needle)) in [
            (
                "a symlink member",
                splice_first_header(tarball(&[("extension.json", 0o644, b"{}")]), |h| {
                    h[156] = b'2'
                }),
                "is not a regular file (type flag '2')",
            ),
            (
                "a directory member",
                splice_first_header(tarball(&[("payload/", 0o755, b"")]), |h| h[156] = b'5'),
                "is not a regular file (type flag '5')",
            ),
            (
                "a pax extension member",
                splice_first_header(tarball(&[("PaxHeaders/0/x", 0o644, b"")]), |h| {
                    h[156] = b'x'
                }),
                "is not a regular file (type flag 'x')",
            ),
            (
                "an escaping member",
                tarball(&[("../evil.sh", 0o755, b"x")]),
                "member '../evil.sh' escapes the package directory",
            ),
            (
                "an absolute member",
                tarball(&[("/absolute.sh", 0o755, b"x")]),
                "member '/absolute.sh' escapes the package directory",
            ),
            (
                "a duplicate member",
                tarball(&[
                    ("extension.json", 0o644, b"{}"),
                    ("extension.json", 0o644, b"{}"),
                ]),
                "a duplicate member 'extension.json'",
            ),
            (
                "data after the end marker",
                {
                    // The junk lands inside the tar stream, after the two
                    // zero blocks — not after the gzip member.
                    let bytes = tarball(&[("extension.json", 0o644, b"{}")]);
                    let mut gz = flate2::read::GzDecoder::new(bytes.as_slice());
                    let mut tar = Vec::new();
                    gz.read_to_end(&mut tar).unwrap();
                    tar.extend_from_slice(b"trailing junk");
                    gzip(&tar)
                },
                "data after its end-of-archive marker",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let staging = tmp.path().join(format!("stage{i}"));
            std::fs::create_dir(&staging).unwrap();
            let err = unpack(&bytes, &staging).unwrap_err();
            assert!(err.to_string().contains(needle), "case {case}: {err}");
            // A mid-archive refusal (duplicate, trailing data) can leave
            // already-written members in the staged dir — it is scratch by
            // design: the installer removes the staged sibling on every
            // error path, so nothing lands at the package target (pinned
            // by `extensions::install`'s no-residue test). The pre-write
            // refusals (type, escape) write nothing at all.
        }
    }

    #[test]
    fn a_truncated_or_corrupt_archive_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        // Truncated payload.
        let full = tarball(&[("extension.json", 0o644, b"{\"kind\":\"command\"}")]);
        let mut cut = full.clone();
        cut.truncate(full.len() - 20);
        let staging = tmp.path().join("stage1");
        std::fs::create_dir(&staging).unwrap();
        // The gzip layer refuses a truncated stream (or the tar layer does,
        // for a truncation that still decodes); either way it is one
        // refusal, never a partial member.
        assert!(unpack(&cut, &staging).is_err());
        // A bad header checksum: flip a name byte after checksumming.
        let mut tar = {
            let mut gz = flate2::read::GzDecoder::new(full.as_slice());
            let mut t = Vec::new();
            gz.read_to_end(&mut t).unwrap();
            t
        };
        tar[3] ^= 0x20; // a name bit the checksum no longer covers
        let corrupt = gzip(&tar);
        let staging = tmp.path().join("stage2");
        std::fs::create_dir(&staging).unwrap();
        let err = unpack(&corrupt, &staging).unwrap_err();
        assert!(
            err.to_string().contains("bad header checksum"),
            "{}",
            err.to_string()
        );
        // Not gzip at all.
        let staging = tmp.path().join("stage3");
        std::fs::create_dir(&staging).unwrap();
        let err = unpack(b"not a tarball", &staging).unwrap_err();
        assert!(
            err.to_string().contains("is not valid gzip"),
            "{}",
            err.to_string()
        );
    }

    /// The unpack cap: a tarball of more than [`MAX_MEMBERS`] members
    /// refuses naming the cap.
    #[test]
    fn the_member_count_is_capped() {
        let members: Vec<(&str, u32, &[u8])> = (0..MAX_MEMBERS + 1)
            .map(|i| {
                let name: &'static str = Box::leak(format!("m{i}").into_boxed_str());
                (name, 0o644, b"x" as &[u8])
            })
            .collect();
        let bytes = tarball(&members);
        let err = unpack_into(&bytes).unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("more than {MAX_MEMBERS} members")),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn the_unpack_helper_reads_names_back() {
        let bytes = tarball(&[("extension.json", 0o644, b"{}")]);
        assert_eq!(
            unpack_into(&bytes).unwrap(),
            vec!["extension.json".to_string()]
        );
    }

    /// The decompression bomb: an archive that unpacks past
    /// [`MAX_UNPACKED_BYTES`] refuses at the read itself — the stream
    /// reads through the bounded read, so the refusal fires while the
    /// data still arrives (one byte past the cap is all it costs), never
    /// after the whole stream sits in memory and never at the
    /// filesystem. The bomb is `MAX_UNPACKED_BYTES + 1` zero bytes,
    /// ~500 KiB compressed: the smallest honest bomb that crosses the
    /// cap.
    #[test]
    fn a_decompression_bomb_refuses_at_the_read() {
        use std::io::Write as _;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let zeros = [0u8; 64 * 1024];
        let mut left = MAX_UNPACKED_BYTES + 1;
        while left > 0 {
            let take = (left.min(zeros.len() as u64)) as usize;
            gz.write_all(&zeros[..take]).unwrap();
            left -= take as u64;
        }
        let bomb = gz.finish().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("stage");
        std::fs::create_dir(&staging).unwrap();
        let err = unpack(&bomb, &staging).unwrap_err();
        assert!(
            err.to_string().contains(&format!(
                "decompresses past the {MAX_UNPACKED_BYTES}-byte cap"
            )),
            "{}",
            err
        );
        // Nothing landed: no member ever parsed — the cap is upstream of
        // the member loop.
        assert!(std::fs::read_dir(&staging).unwrap().next().is_none());
    }

    /// The attacker-controlled member size is bounded in u64 before any
    /// usize narrowing: a header declaring more bytes than the capped
    /// stream holds refuses naming the declared value, and the refusal
    /// compares the u64 the header carried — no truncation to the
    /// platform's usize happens on the way to the refusal.
    #[test]
    fn a_declared_size_past_the_streams_own_bytes_refuses_naming_it() {
        // 0o77777777777 is the most an 11-digit octal size field can
        // declare: ~8.6 GB, far past a tarball the read cap allows.
        const DECLARED: u64 = 0o77777777777;
        let bytes = splice_first_header(tarball(&[("extension.json", 0o644, b"{}")]), |h| {
            h[124..136].copy_from_slice(format!("{DECLARED:011o}\0").as_bytes())
        });
        let err = unpack_into(&bytes).unwrap_err();
        assert!(
            err.to_string().contains(&format!(
                "the tarball ends inside member 'extension.json' ({DECLARED} bytes declared"
            )),
            "{err}"
        );
    }

    /// The closed contract's member names are UTF-8, strictly: a name
    /// that is not refuses with the pinned text — no lossy fabrication
    /// the escaping and duplicate checks could be fed through.
    #[test]
    fn a_non_utf8_member_name_refuses() {
        let bytes = splice_first_header(tarball(&[("extension.json", 0o644, b"{}")]), |h| {
            h[3] = 0xFF; // a name byte that is not valid UTF-8
        });
        let err = unpack_into(&bytes).unwrap_err();
        assert!(
            err.to_string()
                .contains("the tarball carries a member whose name is not valid UTF-8"),
            "{err}"
        );
    }

    /// A chmod failure is the member's refusal, never a silent
    /// permission drift: the file exists, but the path to it lost its
    /// search permission — `apply_mode` propagates the io error the
    /// member loop words through `member_refusal`.
    #[cfg(unix)]
    #[test]
    fn a_mode_failure_propagates_as_the_members_refusal() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nosearch");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("x"), b"m").unwrap();
        // A directory with no search permission: the write above is done,
        // and a chmod through this path needs the search bit it lacks.
        // Root-tolerant: CI containers run as root, which ignores
        // permission bits — the chmod then succeeds, and the refusal
        // this test drives cannot be produced there.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o600)).unwrap();
        if let Err(err) = apply_mode(&dir.join("x"), 0o755) {
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::PermissionDenied,
                "a real chmod failure: {err}"
            );
        }
        // Restore so the tempdir's cleanup can walk back through.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The committed `greet-1.2.3-darwin-arm64.tar.gz` fixture — real
    /// `tar(1)` output in the closed contract's shape (two regular
    /// members, ustar, no pax/dir entries), the same fixture
    /// `tests/e2e_extensions.rs` serves to the real binary: the in-tree
    /// reader must accept what a real producer writes, not only its own
    /// encoder's bytes.
    #[test]
    fn the_committed_tarball_fixture_unpacks() {
        // include_bytes: file-relative at compile time, like the releases
        // fixture the upgrade tests read.
        const FIXTURE: &[u8] =
            include_bytes!("../../tests/fixtures/extension/greet-1.2.3-darwin-arm64.tar.gz");
        assert_eq!(
            unpack_into(FIXTURE).unwrap(),
            vec!["extension.json".to_string(), "greet.sh".to_string()]
        );
        #[cfg(unix)]
        {
            // The executable lands with its exec bit from the member mode.
            let tmp = tempfile::tempdir().unwrap();
            let staging = tmp.path().join("stage");
            std::fs::create_dir(&staging).unwrap();
            unpack(FIXTURE, &staging).unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(staging.join("greet.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    /// Test-only: patch the first member's header block inside a built
    /// tarball — the one splice helper for every test that corrupts a
    /// member (a flipped type flag, a rewritten size, a non-UTF-8 name
    /// byte), restamped through the canonical checksum helper the
    /// reader verifies by.
    fn splice_first_header(bytes: Vec<u8>, patch: impl FnOnce(&mut [u8])) -> Vec<u8> {
        // Re-build through the raw tar: decode gzip, patch the first
        // header, restamp its checksum, re-encode.
        let mut gz = flate2::read::GzDecoder::new(bytes.as_slice());
        let mut tar = Vec::new();
        gz.read_to_end(&mut tar).unwrap();
        patch(&mut tar[..HEADER_BYTES]);
        set_ustar_checksum(&mut tar[..HEADER_BYTES]);
        gzip(&tar)
    }
}
