//! The extension-fixture kit for the CLI e2e suites (the e2e twin of the
//! unit suites' `extensions::testkit`, ADR-0429 §6): a package installed
//! under a tempdir `$XDG_CONFIG_HOME` — its manifest, a POSIX `sh` child,
//! or a copied compiled child — and the `run` invocations pinned to that
//! root, so the registry never reads the machine's real extensions dir
//! from a test. Shared by the run-verb mount suite (`e2e_run.rs`) and
//! the fixture-language suite (`e2e_extension_fixtures.rs`): one kit,
//! so the install and isolation policy cannot drift between them.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use super::{run_cli, tempdir, CliRunOptions};

/// The `run` verb's static synopsis — the line every verb-scope usage
/// error prints (the discovery error, the missing-name error, a flag at
/// the name position, a registry load refusal such as a collision).
pub const RUN_USAGE: &str = "Usage: relate run <name> [flags]";

/// The `run` verb's static help hint — the closing line of every
/// verb-scope usage error.
pub const RUN_HINT: &str = "Run 'relate run --help' for more information.";

/// A tempdir `$XDG_CONFIG_HOME` plus its string form for the env
/// override — every test's isolated machine state.
pub fn xdg_root() -> (tempfile::TempDir, String) {
    let tmp = tempdir();
    let path = tmp
        .path()
        .to_str()
        .expect("the tempdir path is UTF-8")
        .to_string();
    (tmp, path)
}

/// The extensions dir under `xdg` (`<xdg>/relate/extensions/`,
/// ADR-0429 §2) — the one join the kit's installs and every suite's
/// assertions read, so the package-dir fact cannot drift between
/// writing it and checking it.
pub fn extensions_dir(xdg: &Path) -> PathBuf {
    xdg.join("relate").join("extensions")
}

/// Install a package directory and its manifest under `xdg`'s extensions
/// dir (`<xdg>/relate/extensions/<package>/`, ADR-0429 §2) and
/// return the package directory: the caller attaches the executable
/// child it declares.
pub fn install_package(xdg: &Path, package: &str, manifest: &str) -> PathBuf {
    let dir = extensions_dir(xdg).join(package);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extension.json"), manifest).unwrap();
    dir
}

/// The canonical bash MCP client kit the shipped example inlines
/// (`docs/api/extensions/examples/mcp-client.sh`, ADR-0447 §4) — the one
/// text, read here rather than copied, so the fixtures prove the kit
/// package authors get. Its shebang and sourcing note are the sourceable
/// file's header; a package carries the body, which starts at the kit's
/// own first line ([`MCP_KIT_START`]).
const MCP_CLIENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/api/extensions/examples/mcp-client.sh"
));

/// The line the kit's body starts at — past `mcp-client.sh`'s header. The
/// same boundary `scripts/check-extension-examples.sh` pins the example's
/// inline against.
const MCP_KIT_START: &str = "# The MCP revision this kit speaks";

/// The bash MCP kit the fixture children inline (ADR-0447 §4): the
/// canonical file past its header, i.e. the exact text a package carries.
pub fn mcp_kit() -> &'static str {
    let at = MCP_CLIENT
        .find(MCP_KIT_START)
        .expect("mcp-client.sh carries the kit below its header");
    &MCP_CLIENT[at..]
}

/// `body` as a fixture child's whole script: the bash MCP kit, the
/// `initialize` handshake under `name`, then `body` — the test's own
/// `tools/call`s. One composition, so every fixture opens the session the
/// same way.
pub fn mcp_child_body(name: &str, body: &str) -> String {
    format!("{}\nmcp_init {name} 0.1.0\n{body}", mcp_kit())
}

/// Write the package's shell child (`run.sh`) into its package directory and
/// make it executable where the platform has the exec bit — the unit kit's
/// `sh_fixture` pattern, driven through the real binary. The shebang is
/// bash: the MCP kit's JSON scratches use `${…//…}` and `local`, and the
/// wire's language proof is the shell itself, not any one shell.
pub fn install_sh_child(package_dir: &Path, body: &str) -> PathBuf {
    let path = package_dir.join("run.sh");
    std::fs::write(&path, format!("#!/usr/bin/env bash\n{body}")).unwrap();
    make_executable(&path);
    path
}

/// One built fixture bin and the file name its manifest declares: Cargo's
/// path for this test run, and the `.exe` suffix kept on Windows so one
/// manifest builder serves every lane.
fn fixture_bin(path: &str) -> (PathBuf, String) {
    let path = PathBuf::from(path);
    let name = child_file_name(&path);
    (path, name)
}

/// The built `mcp-extension-fixture` bin (the mount's compiled child,
/// ADR-0442) — the compiled half of the e2e language pair.
pub fn mcp_extension_fixture_bin() -> (PathBuf, String) {
    fixture_bin(env!("CARGO_BIN_EXE_mcp-extension-fixture"))
}

/// Copy a built executable into its package directory under its own
/// platform file name and return that name — the compiled-child install
/// (the Rust half of the e2e language pair): the manifest's
/// `executable` declares the returned name. The exec bit is forced the
/// way the `sh` child's is, so a spawned child is always executable.
pub fn install_child_file(package_dir: &Path, built: &Path) -> String {
    let name = child_file_name(built);
    let path = package_dir.join(&name);
    std::fs::copy(built, &path).unwrap();
    make_executable(&path);
    name
}

/// The file name a built executable carries — the value the manifest's
/// `executable` declares for a copied compiled child. One derivation:
/// the manifest builders, the copy, and the importer kit's
/// manifest-before-install write read the same fact. `pub` for the
/// importer kit, which patches its committed manifest's executable to
/// the name the child will land under before the install writes it.
pub fn child_file_name(built: &Path) -> String {
    built
        .file_name()
        .expect("the built child has a file name")
        .to_str()
        .expect("the built child's file name is UTF-8")
        .to_string()
}

/// Make `path` executable where the platform has the exec bit — the
/// install half of an executable child.
#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = std::fs::metadata(path).unwrap().permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(path, mode).unwrap();
}

/// Make `path` executable where the platform has the exec bit: a no-op
/// where it does not (a copied `.exe` is executable by extension).
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// `args` with the XDG pinned to `xdg` — the isolated machine state
/// every `run` invocation reads.
pub fn run_in_xdg(args: &[&str], xdg: &str) -> (ExitStatus, String, String) {
    run_cli(CliRunOptions::new(args).with_envs(&[("XDG_CONFIG_HOME", xdg)]))
}

/// One ustar header for a regular member of `payload` carrying `mode` —
/// the e2e tamper's builder (the spliced smuggled member) and the pack
/// helper's twin half, in the one shared kit. The unit suites' full
/// builder lives in the crate's `extensions::testtar` (a test target
/// cannot reach a `cfg(test)` module, and the crate cannot reach the
/// test dir).
#[cfg(feature = "extension-install")]
pub fn tar_member_header(name: &str, payload: &[u8], mode: u32) -> Vec<u8> {
    let mut block = vec![0u8; 512];
    block[..name.len()].copy_from_slice(name.as_bytes());
    block[100..108].copy_from_slice(format!("{mode:07o}\0").as_bytes());
    block[108..116].copy_from_slice(b"0000000\0");
    block[116..124].copy_from_slice(b"0000000\0");
    block[124..136].copy_from_slice(format!("{:011o}\0", payload.len()).as_bytes());
    block[136..148].copy_from_slice(b"00000000000\0");
    block[156] = b'0';
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let sum: u64 = block
        .iter()
        .enumerate()
        .map(|(i, b)| u64::from(if (148..156).contains(&i) { 0x20 } else { *b }))
        .sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    block
}

/// The ustar tar bytes (uncompressed) for `members`, each payload padded
/// to the block size, the two zero end blocks appended.
#[cfg(feature = "extension-install")]
fn ustar_tar(members: &[(&str, u32, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, mode, payload) in members {
        tar.extend_from_slice(&tar_member_header(name, payload, *mode));
        tar.extend_from_slice(payload);
        let pad = payload.len().div_ceil(512) * 512 - payload.len();
        tar.extend(std::iter::repeat_n(0u8, pad));
    }
    tar.extend([0u8; 1024]);
    tar
}

/// Pack the ADR-0431 §2 distribution tarball — the release workflow's
/// pack stage (`release-extensions.yml`) in miniature: `extension.json`
/// (the manifest bytes) plus the executable, regular members, the
/// executable under its own file name with its exec bit. The manifest's
/// `executable` names the member (the distribution table's one
/// convention), so the caller passes the built binary its manifest
/// declares.
#[cfg(feature = "extension-install")]
pub fn pack_extension_tarball(manifest: &[u8], executable: &Path) -> Vec<u8> {
    use std::io::Write as _;
    let name = child_file_name(executable);
    let binary = std::fs::read(executable).expect("the built executable is readable");
    let tar = ustar_tar(&[
        ("extension.json", 0o644, manifest),
        (&name, 0o755, binary.as_slice()),
    ]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}
