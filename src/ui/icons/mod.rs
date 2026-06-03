use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

mod freedesktop;

use freedesktop::{build_index, compute_icon_uri, has_entry};

/// Resolves a process to an icon URI from freedesktop `.desktop` files and the
/// system icon theme.
///
/// The expensive work — building the `.desktop` index and the per-icon theme
/// `stat()` scans — runs on a worker thread so it never blocks the UI. A cache
/// miss enqueues a request and renders a placeholder; the icon pops in once the
/// worker answers and the next UI poll reads the filled cache. Results persist
/// to `~/.cache/rproc/icons.tsv`, so a warm cache serves most rows from memory
/// without touching the worker.
pub struct Resolver {
    /// Spawned lazily on the first [`Resolver::pump`].
    worker: Option<Worker>,
    /// `.desktop` index keys, reported by the worker once built. `None` until
    /// then — see [`Resolver::has_desktop_entry`].
    index_keys: Option<HashSet<String>>,
    /// `(proc_name, exe_path)` → resolved URI. A hit is a single map lookup with
    /// no worker round-trip. Seeded from disk, filled by worker responses.
    uri_cache: HashMap<String, Option<String>>,
    /// Keys handed to the worker and awaiting a response, so each is enqueued
    /// only once however many frames render before the answer lands.
    pending: HashSet<String>,
    /// Set once `uri_cache` diverges from disk. Gates the write in [`save_persistent`].
    dirty: bool,
    last_flush: Instant,
}

/// Dropping this closes `tx`, ending the worker's `recv()` loop.
struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Response>,
}

enum Request {
    Resolve {
        key: String,
        proc_name: String,
        exe_path: String,
    },
}

enum Response {
    IndexReady(HashSet<String>),
    Resolved { key: String, uri: Option<String> },
}

/// How long a persisted cache stays trusted before a full rescan. Icons move
/// rarely, so a weekly rebuild keeps newly installed apps picked up without
/// re-scanning every launch.
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Minimum gap between throttled disk flushes while the app runs. Bounds the
/// work an abrupt kill can lose to a few seconds of newly resolved icons.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

impl Resolver {
    pub fn new() -> Self {
        let mut r = Self {
            worker: None,
            index_keys: None,
            uri_cache: HashMap::new(),
            pending: HashSet::new(),
            dirty: false,
            last_flush: Instant::now(),
        };
        r.load_persistent();
        r
    }

    /// Drains the worker's results and spawns it on first call. Must run once
    /// per UI poll before any `icon_uri` / `has_desktop_entry` query.
    pub fn pump(&mut self) {
        if self.worker.is_none() {
            self.worker = Some(spawn_worker());
        }
        while let Ok(resp) = self.worker.as_ref().unwrap().rx.try_recv() {
            match resp {
                Response::IndexReady(keys) => self.index_keys = Some(keys),
                Response::Resolved { key, uri } => {
                    self.pending.remove(&key);
                    self.uri_cache.insert(key, uri);
                    self.dirty = true;
                }
            }
        }
    }

    pub fn icon_uri(&mut self, proc_name: &str, exe_path: &str) -> Option<String> {
        let key = make_key(proc_name, exe_path);
        if let Some(cached) = self.uri_cache.get(&key) {
            return cached.clone();
        }
        // Miss: hand off to the worker and render a placeholder until it answers.
        // `pending` keeps a key in flight from being re-enqueued every frame.
        if let Some(worker) = self.worker.as_ref()
            && self.pending.insert(key.clone())
        {
            let _ = worker.tx.send(Request::Resolve {
                key,
                proc_name: proc_name.to_string(),
                exe_path: exe_path.to_string(),
            });
        }
        None
    }

    /// Seeds `uri_cache` from `~/.cache/rproc/icons.tsv` when the file is
    /// younger than [`CACHE_TTL`]. Positive entries are validated against the
    /// filesystem once here — an app may have been updated or removed between
    /// runs — and dead ones are dropped so they get recomputed this session.
    /// A stale (or unreadable) file is simply ignored, which forces a fresh
    /// scan that rewrites the file: that is the weekly refresh.
    fn load_persistent(&mut self) {
        let Some(path) = cache_path() else { return };
        if let Ok(meta) = fs::metadata(&path)
            && let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > CACHE_TTL
        {
            return;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        for line in content.lines() {
            let Some((key, uri)) = line.split_once('\t') else {
                continue;
            };
            let value = if uri.is_empty() {
                None
            } else if uri
                .strip_prefix("file://")
                .is_some_and(|p| Path::new(p).exists())
            {
                Some(uri.to_string())
            } else {
                // Icon file vanished since it was cached → drop it and let this
                // session recompute. Mark dirty so the cleaned set is written back.
                self.dirty = true;
                continue;
            };
            self.uri_cache.insert(key.to_string(), value);
        }
    }

    /// Writes `uri_cache` back to disk so the next launch skips the theme scan.
    /// Cheap (a few KB) and a no-op when nothing changed since the last write.
    /// Clears `dirty` so repeated calls don't rewrite unchanged data.
    pub fn save_persistent(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(path) = cache_path() {
            let _ = fs::write(&path, serialize_cache(&self.uri_cache));
        }
        self.dirty = false;
        self.last_flush = Instant::now();
    }

    /// Throttled flush meant to be called every frame. `on_exit` is the clean
    /// path, but it only fires on an orderly window close — a SIGTERM, session
    /// logout, or compositor teardown skips it and would otherwise discard every
    /// icon resolved this session. Writing at most once per [`FLUSH_INTERVAL`]
    /// caps that loss at a few seconds while staying off the hot path.
    pub fn flush_if_due(&mut self) {
        if self.dirty && self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.save_persistent();
        }
    }

    pub fn has_desktop_entry(&self, proc_name: &str, exe_path: &str) -> bool {
        // Before the index arrives everything reports "no entry" (background
        // section), then reshuffles into Apps once the keys land — a frame or
        // two on a cold start.
        self.index_keys
            .as_ref()
            .is_some_and(|keys| has_entry(keys, proc_name, exe_path))
    }
}

fn spawn_worker() -> Worker {
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let (res_tx, res_rx) = mpsc::channel::<Response>();
    std::thread::Builder::new()
        .name("icon-resolver".to_string())
        .spawn(move || {
            let index = build_index();
            if res_tx
                .send(Response::IndexReady(index.keys().cloned().collect()))
                .is_err()
            {
                return;
            }
            // Memoizes `icon_name` → URI so processes sharing an icon pay the
            // theme stat() scan once.
            let mut icon_cache: HashMap<String, Option<String>> = HashMap::new();
            while let Ok(Request::Resolve {
                key,
                proc_name,
                exe_path,
            }) = req_rx.recv()
            {
                let uri = compute_icon_uri(&index, &mut icon_cache, &proc_name, &exe_path);
                if res_tx.send(Response::Resolved { key, uri }).is_err() {
                    break;
                }
            }
        })
        .expect("failed to spawn icon-resolver thread");
    Worker {
        tx: req_tx,
        rx: res_rx,
    }
}

/// Decode an icon file and downscale it to `size`×`size` device pixels. Process
/// rows render icons at ~16 px, but themed icons on disk are often 48–256 px (or
/// scalable SVG); keeping them at native resolution made the resident image
/// cache balloon to tens of MB. Rasterizing to display size up front caps each
/// cached icon at a couple of KB. Returns `None` on any decode error.
pub fn decode_scaled(path: &Path, size: u32) -> Option<slint::Image> {
    let svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if svg {
        decode_svg(path, size)
    } else {
        decode_raster(path, size)
    }
}

fn buffer_from_bytes(src: &[u8], size: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
    let dst = buf.make_mut_bytes();
    if dst.len() != src.len() {
        return None;
    }
    dst.copy_from_slice(src);
    Some(buf)
}

fn decode_raster(path: &Path, size: u32) -> Option<slint::Image> {
    let img = image::open(path).ok()?.to_rgba8();
    let scaled = image::imageops::resize(&img, size, size, image::imageops::FilterType::Triangle);
    let buf = buffer_from_bytes(scaled.as_raw(), size)?;
    Some(slint::Image::from_rgba8(buf))
}

fn decode_svg(path: &Path, size: u32) -> Option<slint::Image> {
    let data = fs::read(path).ok()?;
    // Default options keep an empty font database — icons rarely carry text, and
    // loading the system fonts here would map tens of MB just to draw a glyph.
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let ts = tree.size();
    let scale = size as f32 / ts.width().max(ts.height()).max(1.0);
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let buf = buffer_from_bytes(pixmap.data(), size)?;
    Some(slint::Image::from_rgba8_premultiplied(buf))
}

/// `proc_name` and `exe_path` joined by a NUL (which occurs in neither), so
/// distinct pairs never collide on a shared prefix.
fn make_key(proc_name: &str, exe_path: &str) -> String {
    let mut key = String::with_capacity(proc_name.len() + exe_path.len() + 1);
    key.push_str(proc_name);
    key.push('\0');
    key.push_str(exe_path);
    key
}

/// Path to the persisted icon cache under the shared `~/.cache/rproc` dir.
fn cache_path() -> Option<PathBuf> {
    crate::daemon::storage::cache_dir()
        .ok()
        .map(|d| d.join("icons.tsv"))
}

/// Renders `uri_cache` as the on-disk TSV: one `key\turi` line per entry, empty
/// `uri` for a negative result. Entries whose key or uri contain a tab/newline
/// are skipped — they'd corrupt the line format and never occur for real
/// process names or icon paths.
fn serialize_cache(uri_cache: &HashMap<String, Option<String>>) -> String {
    let mut buf = String::new();
    for (key, value) in uri_cache {
        let uri = value.as_deref().unwrap_or("");
        if key.contains(['\t', '\n']) || uri.contains(['\t', '\n']) {
            continue;
        }
        buf.push_str(key);
        buf.push('\t');
        buf.push_str(uri);
        buf.push('\n');
    }
    buf
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
