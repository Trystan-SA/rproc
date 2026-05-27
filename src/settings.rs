use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Shared, lock-free settings handle. Cloning is cheap (just an Arc bump);
/// the sampler thread keeps one clone and reads it each tick, the UI thread
/// writes to it.
#[derive(Clone)]
pub struct Settings {
    refresh_ms: Arc<AtomicU64>,
    /// Whether the background daemon that persists the last 60 s of metrics
    /// should run. Persisted so the choice survives restarts — otherwise the
    /// GUI would respawn the daemon on every launch regardless.
    daemon_enabled: Arc<AtomicBool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_ms: Arc::new(AtomicU64::new(DEFAULT_REFRESH_MS)),
            daemon_enabled: Arc::new(AtomicBool::new(true)),
        }
    }
}

pub const DEFAULT_REFRESH_MS: u64 = 1000;
pub const MIN_REFRESH_MS: u64 = 100;
pub const MAX_REFRESH_MS: u64 = 30_000;

/// Curated presets surfaced as quick-pick buttons in the Settings page.
pub const REFRESH_PRESETS: &[(u64, &str)] = &[
    (250, "4× / s"),
    (500, "2× / s"),
    (1000, "1× / s"),
    (2000, "Every 2 s"),
    (5000, "Every 5 s"),
    (10_000, "Every 10 s"),
];

impl Settings {
    /// Load persisted settings from disk, falling back to defaults for any
    /// key that's missing or unparseable. Never fails: a missing or corrupt
    /// config just yields defaults.
    pub fn load() -> Self {
        let settings = Self::default();
        let Ok(path) = config_path() else {
            return settings;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return settings;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "daemon_enabled" {
                settings
                    .daemon_enabled
                    .store(matches!(value.trim(), "true" | "1"), Ordering::Relaxed);
            }
        }
        settings
    }

    pub fn refresh_ms(&self) -> u64 {
        self.refresh_ms.load(Ordering::Relaxed)
    }

    pub fn set_refresh_ms(&self, ms: u64) {
        self.refresh_ms
            .store(ms.clamp(MIN_REFRESH_MS, MAX_REFRESH_MS), Ordering::Relaxed);
    }

    /// Get the underlying Arc so the sampler thread can read updates
    /// without going through the Settings wrapper.
    pub fn refresh_handle(&self) -> Arc<AtomicU64> {
        self.refresh_ms.clone()
    }

    pub fn daemon_enabled(&self) -> bool {
        self.daemon_enabled.load(Ordering::Relaxed)
    }

    /// Flip the daemon toggle and persist the new value immediately. The
    /// caller is responsible for actually spawning/stopping the daemon.
    pub fn set_daemon_enabled(&self, enabled: bool) {
        self.daemon_enabled.store(enabled, Ordering::Relaxed);
        self.save();
    }

    /// Persist the current settings to disk. Best-effort: any failure is
    /// logged to stderr but never propagates.
    fn save(&self) {
        let Ok(path) = config_path() else {
            return;
        };
        let body = format!(
            "daemon_enabled={}\n",
            self.daemon_enabled.load(Ordering::Relaxed)
        );
        if let Err(e) = std::fs::write(&path, body) {
            eprintln!("rproc: failed to save settings: {e}");
        }
    }
}

/// Path to the persisted config file, following the XDG base-dir spec
/// (`$XDG_CONFIG_HOME/rproc/config`, falling back to `~/.config/rproc/config`).
/// Creates the parent directory if needed.
fn config_path() -> std::io::Result<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME or XDG_CONFIG_HOME")
        })?;
    let dir = base.join("rproc");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config"))
}
