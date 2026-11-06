//! The pinned, fail-closed data-set fetch the testing suites share
//! (ADR-0243): the `data.sha256` pin-table parse, the
//! streamed sha256, the verified download, and the per-data-set
//! lock-and-stamp ensure a workload wraps around its own extraction.
//!
//! Nothing is vendored: every data set is downloaded on first use,
//! checked against its committed pin, and re-verified on every run. The
//! cache layout is this module's — a workload only names its files:
//!
//! ```text
//! <root>/archives/<archive>   the archive as downloaded
//! <root>/<name>               the extracted data set (file or directory)
//! <root>/.<name>.ok           stamp holding the archive's sha256
//! <root>/.<name>.lock         per-data-set ensure lock (fs4, exclusive)
//! ```
//!
//! A workload keeps what only it knows: its cache root, its archive and
//! pin-table paths, its extraction, and the text of its offline refusal —
//! which names the command its user runs.
//!
//! A stamp is only believed when the target it describes exists in its
//! [`Target`] kind (a file, or a directory tree) and `.<name>.ok` holds that
//! data set's pinned sha256, trimmed. That is the whole of the fast path's
//! evidence: the stamp records which archive was verified and extracted, not
//! the extracted bytes, so a target rewritten behind its stamp is not
//! re-hashed on the next run — a different pin, or a missing target, sends
//! [`ensure`] back through the lock, the archive verify and the extractor.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::hex;

/// A whole archive at a slow link is still one request: the download's
/// global timeout, from the first connect to the last body byte.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Why a pinned fetch is not available. Every fallible step names the
/// reason a user can act on, typed so callers match on it instead of
/// sniffing the message.
#[derive(Debug)]
pub enum FetchError {
    /// The pin table is malformed: a line that is not
    /// `<sha256> <name>`, a pin that is not 64 lowercase hex characters,
    /// or a table naming nothing.
    PinTable {
        /// The pin table, as the refusal names it.
        path: PathBuf,
        /// The offending line, where the table has one.
        line: Option<usize>,
        detail: String,
    },
    /// A downloaded archive's sha256 is not its pin: it was discarded.
    PinMismatch {
        /// The URL the archive was downloaded from.
        url: String,
        expected: String,
        got: String,
    },
    /// The pin table names no pin for a file the data set needs.
    PinMissing {
        /// The pin table, as the refusal names it.
        path: PathBuf,
        /// The file the table does not name.
        name: String,
    },
    /// The response body is larger than the caller's per-download cap.
    TooLarge { url: String, limit: u64 },
    /// The downloader is https only: the URL, or a redirect hop's, is
    /// plain http.
    HttpsOnly { url: String },
    /// The HTTP fetch failed.
    Network { url: String, detail: String },
    /// A filesystem step failed.
    Io {
        what: String,
        source: std::io::Error,
    },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PinTable { path, line, detail } => match line {
                Some(line) => write!(f, "{}:{line}: {detail}", path.display()),
                None => write!(f, "{}: {detail}", path.display()),
            },
            Self::PinMismatch { url, expected, got } => write!(
                f,
                "{url}: sha256 {got} does not match the pinned {expected}; the archive was \
                 discarded"
            ),
            Self::PinMissing { path, name } => {
                write!(f, "{}: the pin table does not name {name}", path.display())
            }
            Self::TooLarge { url, limit } => write!(
                f,
                "fetching {url}: the response is larger than the {limit} byte limit; the \
                 download was discarded"
            ),
            Self::HttpsOnly { url } => write!(f, "fetching {url}: downloads are https only"),
            Self::Network { url, detail } => write!(f, "fetching {url}: {detail}"),
            Self::Io { what, source } => write!(f, "{what}: {source}"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The suites whose errors are human-readable messages (query-latency's
/// `Result<T, String>`) convert a fetch refusal into its text, so the
/// shared kit fits the suite's own error convention.
impl From<FetchError> for String {
    fn from(err: FetchError) -> Self {
        err.to_string()
    }
}

/// The failure of an I/O step, naming what it was doing.
fn io_error(what: String, source: std::io::Error) -> FetchError {
    FetchError::Io { what, source }
}

/// One pinned file: its name and its sha256 — one line of a pin table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The file's name in the cache.
    pub name: String,
    /// Its sha256, 64 lowercase hex characters.
    pub sha256: String,
}

/// A sha256 pin's spelling: 64 lowercase hex characters — the sha256sum
/// spelling the pin table and the verify path share, validated at parse
/// time so a truncated or non-hex pin is a malformed table, never a
/// permanent download mismatch surfacing after the fetch.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Parse a pin table's text: `#` comment and blank lines aside, every line
/// is `<sha256> <name>` (the sha256sum spelling, whitespace-separated)
/// with the sha256 64 lowercase hex characters — anything else is a
/// malformed table, refused naming `table` and the line. A table naming
/// nothing is refused naming `table` (it has no line to name).
pub fn parse_pins(text: &str, table: &Path) -> Result<Vec<Pin>, FetchError> {
    let mut pins = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line_no = n + 1;
        match line.split_whitespace().collect::<Vec<_>>()[..] {
            [sha256, name] => {
                if !is_sha256_hex(sha256) {
                    return Err(FetchError::PinTable {
                        path: table.to_path_buf(),
                        line: Some(line_no),
                        detail: format!(
                            "a pin's sha256 is 64 lowercase hex characters, got {sha256:?}"
                        ),
                    });
                }
                pins.push(Pin {
                    name: name.to_string(),
                    sha256: sha256.to_string(),
                });
            }
            _ => {
                return Err(FetchError::PinTable {
                    path: table.to_path_buf(),
                    line: Some(line_no),
                    detail: format!("a pin line is `<sha256> <name>`, got {line:?}"),
                })
            }
        }
    }
    if pins.is_empty() {
        return Err(FetchError::PinTable {
            path: table.to_path_buf(),
            line: None,
            detail: "the pin table names no files".to_string(),
        });
    }
    Ok(pins)
}

/// The sha256 a parsed pin table names for `name`: a table that does not
/// name it is its own refusal, naming the table and the file.
pub fn pin_sha256(pins: &[Pin], name: &str, table: &Path) -> Result<String, FetchError> {
    pins.iter()
        .find(|pin| pin.name == name)
        .map(|pin| pin.sha256.clone())
        .ok_or_else(|| FetchError::PinMissing {
            path: table.to_path_buf(),
            name: name.to_string(),
        })
}

/// sha256 of a file, streamed.
pub fn sha256_file(path: &Path) -> Result<String, FetchError> {
    let mut f = File::open(path).map_err(|e| io_error(format!("opening {}", path.display()), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| io_error(format!("reading {}", path.display()), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// What a data set's extraction produces: one file, or a directory tree.
/// The stamp only certifies a target of this kind, and a re-extraction
/// removes the previous one before the extractor runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The extraction produces one file.
    File,
    /// The extraction produces a directory tree.
    Directory,
}

impl Target {
    /// Whether `path` is this kind of target.
    fn present(self, path: &Path) -> bool {
        match self {
            Self::File => path.is_file(),
            Self::Directory => path.is_dir(),
        }
    }

    /// The io call that removes this kind of target.
    fn remove(self, path: &Path) -> std::io::Result<()> {
        match self {
            Self::File => std::fs::remove_file(path),
            Self::Directory => std::fs::remove_dir_all(path),
        }
    }
}

/// One data set the [`ensure`] caches. Named fields, so the archive's
/// name, its URL and its pin — all strings — cannot be transposed at a
/// call site.
#[derive(Debug, Clone, Copy)]
pub struct Dataset<'a> {
    /// The extracted data set's name under the cache root; the stamp and
    /// the lock are named from it.
    pub name: &'a str,
    /// What the extraction produces.
    pub target: Target,
    /// The archive's file name under `<root>/archives`.
    pub archive: &'a str,
    /// Where the archive is downloaded from.
    pub url: &'a str,
    /// The archive's pinned sha256.
    pub sha256: &'a str,
    /// The largest archive body the download accepts.
    pub max_bytes: u64,
}

/// What [`ensure`] may do when the data set's archive is neither cached nor
/// its pin: download it, or refuse with the caller's own error — only the
/// workload knows the command its user runs to fetch its data set.
#[derive(Debug)]
pub enum MissingArchive<F> {
    /// Download the archive from the data set's URL, verified against its pin.
    Download,
    /// Refuse with this error.
    Refuse(F),
}

/// Make sure `dataset` is extracted under `root` and return its path.
///
/// The fast path is the stamp beside the target — `.<name>.ok` holding the
/// archive's sha256, with the target present in its [`Target`] kind — and
/// those two facts are all it certifies (the module doc says what that leaves
/// unverified). Otherwise one [`DatasetLock`] serializes the writers (the same
/// file, so a fetch process and a test in another process agree), the cached
/// archive is verified against its pin — or downloaded when `missing` allows
/// it — the stale target and stamp are removed, `extract` writes the target
/// from the verified archive, and the stamp is written last. Every stamp read
/// other than "no stamp" propagates: an unreadable stamp cannot be treated as
/// "not stamped" without re-extracting, under the extractor's hands, a data
/// set the stamp cannot vouch for.
///
/// The offline refusal is the caller's, carried by
/// [`MissingArchive::Refuse`]: it names the command only the workload knows.
pub fn ensure<F>(
    root: &Path,
    dataset: &Dataset<'_>,
    missing: MissingArchive<F>,
    extract: impl FnOnce(&Path) -> Result<(), F>,
) -> Result<PathBuf, F>
where
    F: From<FetchError>,
{
    let extracted = root.join(dataset.name);
    let stamp = root.join(format!(".{}.ok", dataset.name));
    let stamped = || -> Result<bool, FetchError> {
        if !dataset.target.present(&extracted) {
            return Ok(false);
        }
        match std::fs::read_to_string(&stamp) {
            Ok(text) => Ok(text.trim() == dataset.sha256),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(format!("reading {}", stamp.display()), e)),
        }
    };
    if stamped()? {
        return Ok(extracted);
    }
    std::fs::create_dir_all(root)
        .map_err(|e| io_error(format!("creating {}", root.display()), e))?;
    // One writer per data set: the first through does the work, the rest see
    // the stamp under the lock and take the fast path.
    let lock = DatasetLock::acquire(&root.join(format!(".{}.lock", dataset.name)))?;
    let result = (|| -> Result<PathBuf, F> {
        if stamped()? {
            return Ok(extracted.clone());
        }
        let archive = root.join("archives").join(dataset.archive);
        let cached = archive.is_file() && sha256_file(&archive)? == dataset.sha256;
        if !cached {
            match missing {
                MissingArchive::Download => {
                    download_verified(dataset.url, &archive, dataset.sha256, dataset.max_bytes)?
                }
                MissingArchive::Refuse(error) => return Err(error),
            }
        }
        remove_if_present(&extracted, dataset.target)?;
        remove_if_present(&stamp, Target::File)?;
        extract(&archive)?;
        std::fs::write(&stamp, format!("{}\n", dataset.sha256))
            .map_err(|e| io_error(format!("writing {}", stamp.display()), e))?;
        Ok(extracted.clone())
    })();
    drop(lock);
    result
}

/// One data set's exclusive ensure lock, held from the first stamp re-check to
/// the last stamp write: an exclusive advisory lock over the whole
/// download+extract+stamp serializes the writers, and the guard releases it
/// when it drops — on the error paths too, so a refused fetch never wedges the
/// data set for the next run.
struct DatasetLock(File);

impl DatasetLock {
    /// Take the exclusive advisory lock `path` names — the same file for every
    /// writer, so processes agree — creating it when the cache is fresh.
    fn acquire(path: &Path) -> Result<Self, FetchError> {
        let file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|e| io_error(format!("opening {}", path.display()), e))?;
        fs4::FileExt::lock_exclusive(&file)
            .map_err(|e| io_error(format!("locking {}", path.display()), e))?;
        Ok(Self(file))
    }
}

impl Drop for DatasetLock {
    fn drop(&mut self) {
        // Closing the file releases the advisory lock; the explicit unlock
        // keeps the release on the guard that took it.
        let _ = fs4::FileExt::unlock(&self.0);
    }
}

/// Remove `path` if it exists, as `target`'s kind. Fail closed: a missing
/// path is the expected case, every other error (a permissions failure, a
/// directory where a file was expected) propagates — swallowing one would
/// let a stale extraction or stamp certify what it no longer describes.
fn remove_if_present(path: &Path, target: Target) -> Result<(), FetchError> {
    match target.remove(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(format!("removing {}", path.display()), e)),
    }
}

/// The one HTTP agent the downloader uses: https only — ureq applies that
/// pin to every hop of a redirect chain, so a redirect to plaintext is
/// refused exactly like a plaintext URL — and a whole-archive timeout.
/// Built per download, as the use-once client was before it.
fn download_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .build(),
    )
}

/// Download `url` to `dest` through a temporary [`PartFile`], hashing as it
/// streams; the file is renamed into place only when its sha256 is `want`. The
/// downloader is https only (every redirect hop included) and refuses a body
/// larger than `max_bytes` — the per-call cap the caller sets from the archive
/// it expects — naming the URL and the limit. Either refusal discards the
/// download.
pub fn download_verified(
    url: &str,
    dest: &Path,
    want: &str,
    max_bytes: u64,
) -> Result<(), FetchError> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| io_error(format!("creating {}", parent.display()), e))?;
    let response = download_agent().get(url).call().map_err(|e| match e {
        ureq::Error::RequireHttpsOnly(url) => FetchError::HttpsOnly { url },
        other => FetchError::Network {
            url: url.to_string(),
            detail: other.to_string(),
        },
    })?;
    eprintln!("downloading {url}");
    stream_verified(
        response.into_body().into_reader(),
        dest,
        want,
        max_bytes,
        url,
    )
}

/// A download's `.part` file: the body streams into it, and only the verified
/// file is renamed onto the archive. The guard removes whatever it still holds
/// when it drops, so an abandoned or refused download never leaves its bytes
/// behind.
struct PartFile {
    path: PathBuf,
    /// Set once the file was renamed onto the archive.
    committed: bool,
}

impl PartFile {
    /// The part file one download of `dest` writes: the name carries a process
    /// id AND a per-download sequence number — callers in one process share a
    /// process id, and two of them fetching the same archive concurrently must
    /// not write the same `.part` (one thread's rename removes the file out
    /// from under the other).
    fn beside(dest: &Path) -> Self {
        static PART_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = PART_SEQ.fetch_add(1, Ordering::Relaxed);
        Self {
            path: dest.with_extension(format!("part.{}.{}", std::process::id(), seq)),
            committed: false,
        }
    }

    /// Create the file the body streams into.
    fn create(&self) -> Result<File, FetchError> {
        File::create(&self.path)
            .map_err(|e| io_error(format!("creating {}", self.path.display()), e))
    }

    /// Rename the finished, verified part onto `dest`.
    fn commit(mut self, dest: &Path) -> Result<(), FetchError> {
        std::fs::rename(&self.path, dest)
            .map_err(|e| io_error(format!("renaming into {}", dest.display()), e))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.committed {
            crate::remove_or_report("fetch", &self.path, || std::fs::remove_file(&self.path));
        }
    }
}

/// Stream `source` into `dest`, hashing as it goes: the body lands in a
/// sibling [`PartFile`] that is renamed onto `dest` only when its sha256 is
/// `want`, and a body over `max_bytes` or a hash mismatch discards it. `url`
/// labels the refusals.
fn stream_verified(
    mut source: impl Read,
    dest: &Path,
    want: &str,
    max_bytes: u64,
    url: &str,
) -> Result<(), FetchError> {
    let part = PartFile::beside(dest);
    let mut out = part.create()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = source
            .read(&mut buf)
            .map_err(|e| io_error(format!("reading {url}"), e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max_bytes {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit: max_bytes,
            });
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| io_error(format!("writing {}", part.path.display()), e))?;
    }
    out.sync_all()
        .map_err(|e| io_error(format!("syncing {}", part.path.display()), e))?;
    let got = hex(&hasher.finalize());
    if got != want {
        return Err(FetchError::PinMismatch {
            url: url.to_string(),
            expected: want.to_string(),
            got,
        });
    }
    part.commit(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one 64-lowercase-hex pin every fabricated archive is pinned to;
    /// tests only need a spelling, never a digest they compute themselves.
    const PIN: &str = "967dd4b0733367be1fb2e59e017b0eba941ce1c252fe8e304af42bde45ff3f65";

    /// Put `body` at `root/archives/<archive>` and return its sha256, so a
    /// `Dataset` over it is cached and verified without a download.
    fn cache_archive(root: &Path, archive: &str, body: &[u8]) -> String {
        let path = root.join("archives").join(archive);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        sha256_file(&path).unwrap()
    }

    /// The extractor the ensure tests share: copy the verified archive
    /// into the target — the simplest extraction that produces one.
    fn copy_target(archive: &Path, target: &Path) -> Result<(), TestError> {
        std::fs::copy(archive, target)
            .map(|_| ())
            .map_err(|e| io_error(format!("extracting {}", archive.display()), e).into())
    }

    /// The ensure tests' own error type: [`ensure`] is generic over the
    /// caller's error, and these fixtures never refuse a cached archive, so
    /// their refusal names no command.
    #[derive(Debug)]
    enum TestError {
        /// The offline refusal no fixture returns: every fixture caches its
        /// archive before [`ensure`] runs.
        NotCached,
        /// A refusal the kit itself made — an unreadable stamp, a wrong pin.
        Kit(FetchError),
    }

    impl From<FetchError> for TestError {
        fn from(err: FetchError) -> Self {
            Self::Kit(err)
        }
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::NotCached => f.write_str("the data set is not cached"),
                Self::Kit(err) => err.fmt(f),
            }
        }
    }

    /// The ensure fixtures' missing-archive policy: the archive is cached
    /// before `ensure` runs, so the refusal is built and never returned.
    fn refusal() -> MissingArchive<TestError> {
        MissingArchive::Refuse(TestError::NotCached)
    }

    #[test]
    fn pin_tables_parse_their_lines() {
        let dir = tempfile::tempdir().unwrap();
        let table = dir.path().join("data.sha256");
        let text = "# a comment\n\n\
                    c90febb048ce81b459d9132cececaecd0033a9a530d542d890a18c1afeca3d65  stations.csv\n\
                    1006934d2423ec44ac23dfeb66a4eab9edb0195e8f595ebb5958a46cc2d4d151  links.csv\n";
        let pins = parse_pins(text, &table).unwrap();
        assert_eq!(
            pins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["stations.csv", "links.csv"]
        );
        assert!(pins.iter().all(|p| is_sha256_hex(&p.sha256)));
        assert_eq!(
            pin_sha256(&pins, "links.csv", &table).unwrap(),
            "1006934d2423ec44ac23dfeb66a4eab9edb0195e8f595ebb5958a46cc2d4d151"
        );
        let err = pin_sha256(&pins, "absent.csv", &table).unwrap_err();
        assert!(matches!(err, FetchError::PinMissing { .. }), "{err:?}");
        assert!(
            err.to_string().contains("does not name absent.csv"),
            "{err}"
        );

        let err = parse_pins("one-field-only\n", &table).unwrap_err();
        assert!(
            err.to_string().contains("a pin line is `<sha256> <name>`"),
            "{err}"
        );
        assert!(
            err.to_string().contains("data.sha256:1:"),
            "the refusal names the line: {err}"
        );
        // A truncated (or otherwise non-hex) sha256 is a malformed pin,
        // refused at parse time — never a permanent download mismatch.
        let err = parse_pins("c90febb048  stations.csv\n", &table).unwrap_err();
        assert!(
            err.to_string().contains("64 lowercase hex characters"),
            "{err}"
        );
        assert!(err.to_string().contains("data.sha256:1:"), "{err}");
        // A 64-character lowercase but non-hex pin (g–z are lowercase, not
        // hex) is refused too — the predicate admits exactly the alphabet
        // its error text names.
        let err = parse_pins(
            "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg  stations.csv\n",
            &table,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("64 lowercase hex characters"),
            "{err}"
        );
        let err = parse_pins("# only comments\n\n", &table).unwrap_err();
        assert!(
            err.to_string().contains("the pin table names no files"),
            "{err}"
        );
        assert!(
            err.to_string().contains("data.sha256"),
            "the refusal names the table: {err}"
        );
    }

    #[test]
    fn sha256_file_streams_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// The downloader's https-only pin: the agent refuses a plain http URL,
    /// and ureq applies `https_only` to every redirect hop, so a redirect
    /// to plaintext is the same refusal.
    #[test]
    fn the_downloader_is_https_only() {
        assert!(download_agent().config().https_only());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("archive.gz");
        let url = "http://127.0.0.1:1/archive.gz";
        let err = download_verified(url, &dest, PIN, 1 << 20).unwrap_err();
        match &err {
            FetchError::HttpsOnly { url: got } => assert_eq!(got, url),
            other => panic!("expected an https-only refusal, got {other:?}"),
        }
        assert!(err.to_string().contains(url), "{err}");
        assert!(!dest.exists(), "a refused download never lands");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no part file is left behind"
        );
    }

    /// A body over the per-call cap fails closed with a typed too-large
    /// refusal naming the URL and the limit, and nothing is renamed into
    /// place — the part file's guard removes what the stream wrote.
    #[test]
    fn a_body_over_the_limit_is_refused_naming_url_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("archive.gz");
        let url = "https://example.org/archive.gz";
        let body = vec![0u8; 11];
        let err = stream_verified(std::io::Cursor::new(body), &dest, PIN, 10, url).unwrap_err();
        match &err {
            FetchError::TooLarge { url: got, limit } => {
                assert_eq!(got, url);
                assert_eq!(*limit, 10);
            }
            other => panic!("expected a too-large refusal, got {other:?}"),
        }
        let text = err.to_string();
        assert!(text.contains(url) && text.contains("10"), "{text}");
        assert!(!dest.exists(), "an over-limit body never lands");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the part file is discarded"
        );
    }

    /// A body exactly at the limit is accepted, and a mismatched hash
    /// discards the download instead of renaming it into place.
    #[test]
    fn the_limit_is_inclusive_and_a_mismatch_discards() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("archive.gz");
        std::fs::write(dir.path().join("body"), b"abc").unwrap();
        let got = sha256_file(&dir.path().join("body")).unwrap();
        stream_verified(std::io::Cursor::new(b"abc".to_vec()), &dest, &got, 3, "u").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            2,
            "the archive and the body fixture, and no part file"
        );
        let mismatch = dir.path().join("mismatch");
        let err = stream_verified(
            std::io::Cursor::new(b"abcd".to_vec()),
            &mismatch,
            &got,
            4,
            "u",
        )
        .unwrap_err();
        assert!(matches!(err, FetchError::PinMismatch { .. }), "{err:?}");
        assert!(!mismatch.exists());
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            2,
            "a mismatched download leaves no part file"
        );
    }

    /// Every data set gets its own lock and stamp, and a cached one needs
    /// no network: two data sets in one root do not share either.
    #[test]
    fn one_lock_per_dataset() {
        let root = tempfile::tempdir().unwrap();
        for name in ["alpha", "beta"] {
            let archive = format!("{name}.gz");
            let sha256 = cache_archive(root.path(), &archive, name.as_bytes());
            let dataset = Dataset {
                name,
                target: Target::File,
                archive: &archive,
                url: "https://example.org/archive.gz",
                sha256: &sha256,
                max_bytes: 1 << 20,
            };
            let target = root.path().join(name);
            let got = ensure(root.path(), &dataset, refusal(), |a| {
                copy_target(a, &target)
            })
            .unwrap();
            assert_eq!(got, target);
        }
        for name in ["alpha", "beta"] {
            assert!(root.path().join(format!(".{name}.lock")).is_file());
            assert!(root.path().join(format!(".{name}.ok")).is_file());
        }
    }

    /// Two ensures of one data set serialize on its lock: the extractor
    /// runs once, and the second caller takes the stamped fast path.
    #[test]
    fn concurrent_ensures_extract_once() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path();
        let name = "alpha";
        let archive = format!("{name}.gz");
        let sha256 = cache_archive(root_path, &archive, b"body");
        let dataset = Dataset {
            name,
            target: Target::File,
            archive: &archive,
            url: "https://example.org/archive.gz",
            sha256: &sha256,
            max_bytes: 1 << 20,
        };
        let target = root_path.join(name);
        let extractions = std::sync::atomic::AtomicUsize::new(0);
        let count = &extractions;
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            let first_target = target.clone();
            scope.spawn(move || {
                ensure(root_path, &dataset, refusal(), move |a| {
                    count.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    // Hold the lock while the other caller arrives, so the
                    // two really overlap.
                    release_rx.recv().unwrap();
                    copy_target(a, &first_target)
                })
                .unwrap();
            });
            entered_rx.recv().unwrap();
            let second_target = target.clone();
            let second = scope.spawn(move || {
                ensure(root_path, &dataset, refusal(), move |a| {
                    copy_target(a, &second_target)
                })
                .unwrap()
            });
            release_tx.send(()).unwrap();
            second.join().unwrap();
        });
        assert_eq!(extractions.load(Ordering::SeqCst), 1);
        assert!(root_path.join(format!(".{name}.ok")).is_file());
    }

    /// An unreadable stamp is an error, never "not stamped": the ensure
    /// must not extract over a data set its stamp cannot vouch for.
    #[test]
    fn an_unreadable_stamp_propagates() {
        let root = tempfile::tempdir().unwrap();
        let name = "alpha";
        let archive = format!("{name}.gz");
        let sha256 = cache_archive(root.path(), &archive, b"body");
        let dataset = Dataset {
            name,
            target: Target::File,
            archive: &archive,
            url: "https://example.org/archive.gz",
            sha256: &sha256,
            max_bytes: 1 << 20,
        };
        let target = root.path().join(name);
        std::fs::write(&target, b"stale").unwrap();
        // A directory where the stamp should be: reading it fails with a
        // kind other than NotFound.
        std::fs::create_dir(root.path().join(format!(".{name}.ok"))).unwrap();
        let err = ensure(root.path(), &dataset, refusal(), |a| {
            copy_target(a, &target)
        })
        .unwrap_err();
        assert!(
            matches!(err, TestError::Kit(FetchError::Io { .. })),
            "{err:?}"
        );
        assert!(
            err.to_string().contains(&format!(".{name}.ok")),
            "the refusal names the stamp: {err}"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"stale",
            "the target is untouched"
        );
    }
}
