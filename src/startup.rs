//! Launch timing for `tools/harness/bounce.py`. Off unless `THESIO_BOUNCE_T0`
//! holds the launcher's clock in Unix milliseconds.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn origin() -> Option<f64> {
    static ORIGIN: OnceLock<Option<f64>> = OnceLock::new();
    *ORIGIN.get_or_init(|| std::env::var("THESIO_BOUNCE_T0").ok()?.parse().ok())
}

pub fn mark(label: &str) {
    let Some(origin) = origin() else { return };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() * 1000.;
    eprintln!("bounce {label} {:.1}", now - origin);
}

/// The UI has nothing left to paint or load: the run ends here.
pub fn settled() {
    if origin().is_some() {
        mark("settled");
        std::process::exit(0);
    }
}
