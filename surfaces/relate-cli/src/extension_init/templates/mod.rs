//! The `extension init` templates (ADR-0431 §3): one package skeleton per
//! language, each a manifest, one executable and a README.
//!
//! The executable is the language's **minimal MCP client, generated inline**
//! — the vendored glue (envelope read, JSON-RPC lines, the tool calls) plus a
//! TODO-marked `walk()` the author replaces. Nothing here is a published
//! client dependency to install or manage: the bash template inlines the
//! canonical kit ([`crate::extensions::kit::mcp_kit`]), Python and
//! TypeScript get the transport and the minimal client as generated source,
//! and the Rust template shows the wire kit's usage
//! (`relate_wire::mcp`), which is the kit both sides of the mount
//! already share.
//!
//! Every glue file opens with a header naming the transport contract's home
//! ([`GLUE_SOURCE`]): the contract lives in `docs/api/extensions/spec.md`
//! ("The Transport contract"), so changing the glue means re-scaffolding or
//! copying that section — never a package version to keep in step.
//!
//! Templates are plain text with placeholders ([`NAME_PLACEHOLDER`],
//! [`GLUE_SOURCE_PLACEHOLDER`], [`PROTOCOL_PLACEHOLDER`]) rather than
//! `format!` strings: the bodies carry braces in their own languages (JSON,
//! Python dicts, TypeScript object literals), and a template that had to
//! escape them would be unreadable. [`render`] is the one substitution.

mod py;
mod rust;
mod sh;
mod ts;

/// The package name's placeholder in every template body. Replaced with the
/// `init` invocation's name, which is validated before any template is
/// rendered.
const NAME_PLACEHOLDER: &str = "__NAME__";

/// The glue-source pointer's placeholder: every glue body names the contract
/// it is a copy of, and [`render`] fills in [`GLUE_SOURCE`].
const GLUE_SOURCE_PLACEHOLDER: &str = "__GLUE_SOURCE__";

/// The MCP revision's placeholder: the manifest and the generated clients
/// that spell the revision render [`MCP_PROTOCOL_VERSION`] through it, so a
/// revision bump moves every copy at once — a scaffold can never ship a
/// handshake its own host refuses.
const PROTOCOL_PLACEHOLDER: &str = "__PROTOCOL_VERSION__";

/// The MCP revision this host serves, the value [`PROTOCOL_PLACEHOLDER`]
/// renders — the wire kit's one constant, never a re-typed string.
const MCP_PROTOCOL_VERSION: &str = relate_wire::mcp::MCP_PROTOCOL_VERSION;

/// The header every generated glue file carries: the contract the glue is a
/// copy of, so a contract change is a re-scaffold, not a version bump. The
/// path and the section name are the pointer; both are pinned by
/// [`tests::every_template_names_the_transport_contract`].
const GLUE_SOURCE: &str = "docs/api/extensions/spec.md, \"The Transport contract\"";

/// The one placeholder substitution every template speaks: the package name,
/// the glue-source pointer and the MCP revision, replaced in the template
/// body. One helper (beside the placeholders it replaces), not one per
/// language.
pub(super) fn render(body: &str, name: &str) -> String {
    body.replace(NAME_PLACEHOLDER, name)
        .replace(GLUE_SOURCE_PLACEHOLDER, GLUE_SOURCE)
        .replace(PROTOCOL_PLACEHOLDER, MCP_PROTOCOL_VERSION)
}

/// What one generated file is. The block `init` prints and the tests select
/// by role — never by path spelling — so a template renames its files
/// without reaching into the printer or the assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// `extension.json`: the manifest the host parses.
    Manifest,
    /// A file the executable is built with, but not the glue itself (the
    /// Rust crate's `Cargo.toml`).
    BuildFile,
    /// The executable a package ships: the generated glue, landing executable
    /// where the platform has the exec bit (the installer copies the mode).
    GlueExecutable,
    /// Generated glue that ships as source and is compiled (the Rust crate's
    /// `src/main.rs`) — never itself the executable.
    GlueSource,
    /// The README quickstart.
    Readme,
}

impl Role {
    /// Every role, in the order the enum declares them — the tests' list, so
    /// a new role cannot land unvisited (test-only: the printer reads the
    /// role a file carries, never the vocabulary).
    #[cfg(test)]
    pub(super) const ALL: [Role; 5] = [
        Role::Manifest,
        Role::BuildFile,
        Role::GlueExecutable,
        Role::GlueSource,
        Role::Readme,
    ];

    /// What this file is, for the block `init` prints.
    pub(super) fn description(self) -> &'static str {
        match self {
            Role::Manifest => "the manifest (mount name, MCP revision, flag table)",
            Role::BuildFile => "the crate that builds the executable",
            Role::GlueExecutable => "the executable: generated glue plus your walk()",
            Role::GlueSource => "the child: `serve` plus your walk()",
            Role::Readme => "the quickstart for this package",
        }
    }
}

/// One file of a generated package: its path relative to the package
/// directory, its contents, and what it is.
#[derive(Debug)]
pub(super) struct PackageFile {
    /// The path inside the package directory. The executable always sits at
    /// the top level — a package is the manifest plus one executable
    /// (ADR-0429 §2) — while the Rust template's crate lays its source under
    /// `src/` beside it.
    pub(super) path: String,
    /// The file's whole contents.
    pub(super) contents: String,
    /// What the file is: the manifest, the executable, glue source, a build
    /// file or the README.
    pub(super) role: Role,
}

impl PackageFile {
    /// The one constructor: every file lands with the role its template chose
    /// for it.
    pub(super) fn new(path: impl Into<String>, contents: impl Into<String>, role: Role) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
            role,
        }
    }
}

/// A language `extension init` can scaffold — the closed `--lang`
/// vocabulary. `py` is the default: the widest-spread runtime and the
/// shortest glue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Language {
    /// Bash: the canonical kit, inlined.
    Sh,
    /// Python 3: the generated transport plus a minimal client.
    Py,
    /// TypeScript on Node (22.18+, types stripped natively).
    Ts,
    /// Rust: the wire kit's usage, generated as source.
    Rust,
}

impl Language {
    /// Every value `--lang` accepts, in the order the usage errors list them.
    pub(crate) const ALL: [Language; 4] =
        [Language::Sh, Language::Py, Language::Ts, Language::Rust];

    /// The default language when `--lang` is absent.
    pub(crate) const DEFAULT: Language = Language::Py;

    /// The value's CLI spelling — the one word `--lang` accepts.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Language::Sh => "sh",
            Language::Py => "py",
            Language::Ts => "ts",
            Language::Rust => "rust",
        }
    }

    /// The closed vocabulary, as a usage error spells it
    /// (`sh, py, ts or rust`) — derived from [`Self::ALL`], so the message
    /// cannot fall behind the accepted values.
    pub(crate) fn vocabulary() -> String {
        let words: Vec<&str> = Self::ALL.iter().map(|language| language.as_str()).collect();
        let (last, rest) = words
            .split_last()
            .expect("the language vocabulary is never empty");
        format!("{} or {last}", rest.join(", "))
    }

    /// Parse a `--lang` value; `None` for anything outside the vocabulary —
    /// the caller turns that into the usage error naming the value and the
    /// vocabulary, never a silent fallback to the default.
    pub(crate) fn parse(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.as_str() == word)
    }

    /// The whole package this language scaffolds: the manifest, the
    /// language's own files and the README. One dispatch: each template
    /// module owns its file set and the manifest's executable name.
    pub(super) fn package(self, name: &str) -> Vec<PackageFile> {
        let (executable, mut files) = match self {
            Language::Sh => sh::package(name),
            Language::Py => py::package(name),
            Language::Ts => ts::package(name),
            Language::Rust => rust::package(name),
        };
        files.insert(0, manifest(name, &executable));
        files
    }
}

/// The manifest every generated package carries: the command stratum's
/// fields, the MCP revision this host serves, the sample flag table, and the
/// per-batch commit policy.
///
/// The table carries the one flag every generated walk binds end to end
/// (`--name`, the sample's greeting), because that round trip is the
/// contract an author must get right: the manifest declares the flag, the
/// host resolves its default into the request envelope, the walk binds it as
/// a Cypher parameter, and the read's result feeds the summary. The README
/// and the walk's TODO name the table as the next thing to extend.
fn manifest(name: &str, executable: &str) -> PackageFile {
    let protocol = MCP_PROTOCOL_VERSION;
    let manifest = format!(
        r#"{{
  "kind": "command",
  "name": "{name}",
  "version": "0.1.0",
  "protocol": "{protocol}",
  "flags": [
    {{"kind": "string", "name": "name", "default": "there"}}
  ],
  "commit": "per-batch",
  "executable": "{executable}"
}}
"#,
    );
    PackageFile::new("extension.json", manifest, Role::Manifest)
}

#[cfg(test)]
mod tests;
