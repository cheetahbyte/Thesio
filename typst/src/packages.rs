//! Typst packages, in the folders the typst CLI uses, so the two share them.
//! Looking one up stays on disk and fetching is a separate step, which is the
//! only way a document reaches the network.

use rustc_hash::FxHashMap;
use std::sync::Mutex;
use std::any::Any;
use std::io::{self, Read};
use std::time::{Duration, Instant};
use typst::diag::{PackageError, PackageResult};
pub use typst::syntax::package::PackageSpec;
use typst_kit::downloader::Downloader;
use typst_kit::files::FsRoot;
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};

/// How long a failed download stands. Compiles come with every pause in typing
/// and each would otherwise ask again.
const RETRY_AFTER: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("Thesio/", env!("CARGO_PKG_VERSION"));
const PACKAGE_TIMEOUT: Duration = Duration::from_secs(120);
const PACKAGE_LIMIT: u64 = 200 * 1024 * 1024;

/// Fetches Typst packages and their index for `typst_kit`.
struct PackageDownloader;

impl Downloader for PackageDownloader {
    fn stream(&self, _: &dyn Any, url: &str) -> io::Result<(Option<usize>, Box<dyn Read>)> {
        let request = ureq::get(url).header("Accept", "*/*").header("User-Agent", USER_AGENT).config().timeout_global(Some(PACKAGE_TIMEOUT)).build();
        match request.call() {
            Ok(response) => {
                let body = response.into_body();
                let size = body.content_length().map(|size| size as usize);
                Ok((size, Box::new(body.into_with_config().limit(PACKAGE_LIMIT).reader())))
            }
            Err(ureq::Error::StatusCode(404)) => Err(io::ErrorKind::NotFound.into()),
            Err(ureq::Error::StatusCode(code)) => Err(io::Error::other(format!("the server answered with status {code}"))),
            Err(e) => Err(io::Error::other(format!("check your connection: {e}"))),
        }
    }
}

pub struct Packages {
    system: SystemPackages,
    /// In memory only, so a failure never outlives the app or touches the cache.
    failed: Mutex<FxHashMap<PackageSpec, (Instant, PackageError)>>,
    retry_after: Duration,
}

impl Packages {
    pub fn new() -> Self { Self::from_parts(SystemPackages::new(PackageDownloader), RETRY_AFTER) }

    fn from_parts(system: SystemPackages, retry_after: Duration) -> Self {
        Self { system, failed: Mutex::default(), retry_after }
    }

    /// Where a package is on disk, `None` if it should be fetched, or why it
    /// cannot be had. Makes no request.
    pub(crate) fn find(&self, spec: &PackageSpec) -> PackageResult<Option<FsRoot>> {
        let on_disk = |packages: Option<&FsPackages>| packages.and_then(|packages| packages.obtain(spec));
        if let Some(root) = on_disk(self.system.data()).or_else(|| on_disk(self.system.cache())) { return Ok(Some(root)); }
        if spec.namespace != UniversePackages::NAMESPACE || self.system.cache().is_none() { return Err(PackageError::NotFound(spec.clone())); }
        match self.failed.lock().unwrap().get(spec) {
            Some((at, error)) if at.elapsed() < self.retry_after => Err(error.clone()),
            _ => Ok(None),
        }
    }

    /// Downloads a package into the cache, blocking until it is there or has
    /// failed. typst-kit unpacks into a temporary folder and moves it into
    /// place whole, so a failure leaves nothing a later compile would trust.
    pub fn fetch(&self, spec: &PackageSpec) {
        let result = self.system.obtain(spec);
        let mut failed = self.failed.lock().unwrap();
        match result {
            Ok(_) => { failed.remove(spec); }
            Err(error) => { failed.insert(spec.clone(), (Instant::now(), error)); }
        }
    }
}

impl Default for Packages {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{Compilation, Compiler};
    use std::io::{Cursor, ErrorKind};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SPEC: &str = "@preview/hello:1.0.0";
    const IMPORT: &str = "#import \"@preview/hello:1.0.0\": greet\n#greet\n";
    const MANIFEST: &str = "[package]\nname = \"hello\"\nversion = \"1.0.0\"\nentrypoint = \"lib.typ\"\n";
    const LIB: &str = "#let greet = [Hello from a package]\n";

    /// Answers every request the same way and counts them; never the network.
    struct Fake { answer: Mutex<io::Result<Vec<u8>>>, requests: AtomicUsize }

    impl Fake {
        fn answering(answer: io::Result<Vec<u8>>) -> Arc<Self> { Arc::new(Self { answer: Mutex::new(answer), requests: AtomicUsize::new(0) }) }
        fn offline() -> Arc<Self> { Self::answering(Err(io::Error::other("offline"))) }
        fn requests(&self) -> usize { self.requests.load(Ordering::Relaxed) }
    }

    impl Downloader for Fake {
        fn stream(&self, _: &dyn Any, _: &str) -> io::Result<(Option<usize>, Box<dyn Read>)> {
            self.requests.fetch_add(1, Ordering::Relaxed);
            match &*self.answer.lock().unwrap() {
                Ok(bytes) => Ok((None, Box::new(Cursor::new(bytes.clone())))),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }
    }

    struct Setup { vault: PathBuf, data: PathBuf, cache: PathBuf }

    impl Setup {
        fn new(name: &str, main: &str) -> Self {
            let base = std::env::temp_dir().join(format!("thesio-packages-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let setup = Self { vault: base.join("vault"), data: base.join("data"), cache: base.join("cache") };
            for dir in [&setup.vault, &setup.data, &setup.cache] { std::fs::create_dir_all(dir).unwrap(); }
            std::fs::write(setup.vault.join("main.typ"), main).unwrap();
            setup
        }

        fn compiler(&self, downloader: &Arc<Fake>, retry_after: Duration) -> Compiler {
            let universe = UniversePackages::new(downloader.clone());
            let system = SystemPackages::from_parts(Some(FsPackages::new(&self.data)), Some(FsPackages::new(&self.cache)), universe);
            Compiler::with_packages(&self.vault, Packages::from_parts(system, retry_after).into())
        }

        /// The cache's folders for the package, finished or temporary.
        fn cached(&self) -> Vec<String> {
            let Ok(entries) = std::fs::read_dir(self.cache.join("preview/hello")) else { return vec![] };
            entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect()
        }
    }

    fn place(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("typst.toml"), MANIFEST).unwrap();
        std::fs::write(dir.join("lib.typ"), LIB).unwrap();
    }

    fn archive() -> Vec<u8> {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(vec![], flate2::Compression::default()));
        for (name, text) in [("typst.toml", MANIFEST), ("lib.typ", LIB)] {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, text.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn errors(compilation: &Compilation) -> Vec<&str> {
        compilation.diagnostics.iter().filter(|d| d.error).map(|d| d.message.as_str()).collect()
    }

    fn missing(compilation: &Compilation) -> Vec<String> { compilation.missing.iter().map(ToString::to_string).collect() }

    #[test]
    fn a_cached_package_compiles_with_no_request() {
        let setup = Setup::new("cached", IMPORT);
        place(&setup.cache.join("preview/hello/1.0.0"));
        let downloader = Fake::offline();
        let compilation = setup.compiler(&downloader, RETRY_AFTER).compile("main.typ").unwrap();
        assert_eq!(errors(&compilation), Vec::<&str>::new());
        assert!(compilation.document.is_some() && compilation.missing.is_empty());
        assert_eq!(downloader.requests(), 0);
    }

    #[test]
    fn a_local_package_comes_from_the_data_folder() {
        let setup = Setup::new("local", "#import \"@local/hello:1.0.0\": greet\n#greet\n");
        place(&setup.data.join("local/hello/1.0.0"));
        let downloader = Fake::offline();
        let compilation = setup.compiler(&downloader, RETRY_AFTER).compile("main.typ").unwrap();
        assert_eq!(errors(&compilation), Vec::<&str>::new());
        assert!(compilation.document.is_some());
    }

    #[test]
    fn a_missing_package_is_a_status_and_compiling_makes_no_request() {
        let setup = Setup::new("missing", IMPORT);
        let downloader = Fake::offline();
        let compilation = setup.compiler(&downloader, RETRY_AFTER).compile("main.typ").unwrap();
        assert_eq!(missing(&compilation), [SPEC]);
        assert_eq!(errors(&compilation), Vec::<&str>::new());
        let status = &compilation.diagnostics[0];
        assert_eq!(status.message, format!("Downloading {SPEC}…"));
        assert_eq!((status.path.as_deref(), status.line), (Some("main.typ"), Some(0)));
        assert_eq!(downloader.requests(), 0);
    }

    #[test]
    fn a_fetched_package_compiles_and_needs_no_second_request() {
        let setup = Setup::new("fetched", IMPORT);
        let downloader = Fake::answering(Ok(archive()));
        let mut compiler = setup.compiler(&downloader, RETRY_AFTER);
        let compilation = compiler.compile_fetching("main.typ").unwrap();
        assert_eq!(errors(&compilation), Vec::<&str>::new());
        assert!(compilation.document.is_some() && compilation.missing.is_empty());
        assert_eq!(setup.cached(), ["1.0.0"]);
        compiler.compile("main.typ").unwrap();
        assert_eq!(downloader.requests(), 1);
    }

    #[test]
    fn an_offline_failure_is_reported_once_and_leaves_the_cache_clean() {
        let setup = Setup::new("offline", IMPORT);
        let downloader = Fake::offline();
        let mut compiler = setup.compiler(&downloader, RETRY_AFTER);
        let first = compiler.compile("main.typ").unwrap();
        first.fetch(&first.missing[0]);
        assert!(setup.cached().is_empty(), "{:?}", setup.cached());
        let second = compiler.compile("main.typ").unwrap();
        assert_eq!(errors(&second), ["failed to download package (offline)"]);
        assert!(second.missing.is_empty(), "typing must not ask again at once");
    }

    #[test]
    fn a_failure_is_retried_later_and_can_succeed() {
        let setup = Setup::new("retry", IMPORT);
        let downloader = Fake::offline();
        let mut compiler = setup.compiler(&downloader, Duration::ZERO);
        let first = compiler.compile("main.typ").unwrap();
        first.fetch(&first.missing[0]);
        *downloader.answer.lock().unwrap() = Ok(archive());
        let second = compiler.compile_fetching("main.typ").unwrap();
        assert_eq!(errors(&second), Vec::<&str>::new());
        assert!(second.document.is_some());
    }

    #[test]
    fn a_cut_off_archive_leaves_the_cache_clean() {
        let setup = Setup::new("truncated", IMPORT);
        let mut bytes = archive();
        bytes.truncate(bytes.len() / 2);
        let mut compiler = setup.compiler(&Fake::answering(Ok(bytes)), RETRY_AFTER);
        let first = compiler.compile("main.typ").unwrap();
        first.fetch(&first.missing[0]);
        assert!(setup.cached().is_empty(), "{:?}", setup.cached());
        let found = compiler.compile("main.typ").unwrap();
        assert!(errors(&found)[0].starts_with("failed to decompress package"), "{:?}", errors(&found));
    }

    #[test]
    fn a_package_that_does_not_exist_says_so() {
        let setup = Setup::new("unknown", IMPORT);
        let mut compiler = setup.compiler(&Fake::answering(Err(ErrorKind::NotFound.into())), RETRY_AFTER);
        let first = compiler.compile("main.typ").unwrap();
        first.fetch(&first.missing[0]);
        assert_eq!(errors(&compiler.compile("main.typ").unwrap()), [format!("package not found (searched for {SPEC})")]);
    }

    #[test]
    fn only_universe_packages_download() {
        let setup = Setup::new("namespace", "#import \"@local/hello:1.0.0\": greet\n");
        let compilation = setup.compiler(&Fake::offline(), RETRY_AFTER).compile("main.typ").unwrap();
        assert!(compilation.missing.is_empty());
        assert_eq!(errors(&compilation), ["package not found (searched for @local/hello:1.0.0)"]);
    }
}
