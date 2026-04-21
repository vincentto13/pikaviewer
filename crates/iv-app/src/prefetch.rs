use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use winit::event_loop::EventLoopProxy;

/// Number of prefetch worker threads. With >1, N+1 and N-1 can decode in
/// parallel so backward navigation is pre-warmed even while the forward
/// prefetch is still running.
const WORKER_COUNT: usize = 2;

use iv_core::format::{DecodedImage, PluginRegistry};

use crate::app::AppEvent;

// ── EXIF extraction (moved from app.rs so background worker can use it) ─────

#[derive(Clone, Default)]
pub(crate) struct ExifData {
    pub camera_make:   Option<String>,
    pub camera_model:  Option<String>,
    pub lens_model:    Option<String>,
    pub exposure_time: Option<String>,
    pub f_number:      Option<String>,
    pub iso:           Option<String>,
    pub focal_length:  Option<String>,
    pub date_taken:    Option<String>,
    pub orientation:   u32,
}

pub(crate) fn extract_exif(data: &[u8]) -> ExifData {
    let mut out = ExifData::default();

    let reader = exif::Reader::new();
    let mut cursor = std::io::Cursor::new(data);
    let Ok(exif) = reader.read_from_container(&mut cursor) else {
        return out;
    };

    let str_field = |tag| -> Option<String> {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
    };

    out.orientation = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Short(v) if !v.is_empty() => Some(u32::from(v[0])),
            _ => None,
        })
        .unwrap_or(0);

    out.camera_make  = str_field(exif::Tag::Make);
    out.camera_model = str_field(exif::Tag::Model);
    out.lens_model   = str_field(exif::Tag::LensModel);
    out.date_taken   = str_field(exif::Tag::DateTimeOriginal);

    out.exposure_time = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Rational(v) if !v.is_empty() => {
                let r = v[0];
                if r.num == 0 {
                    None
                } else if r.denom > r.num {
                    let reduced = r.denom / r.num;
                    Some(format!("1/{reduced} s"))
                } else {
                    Some(format!("{:.1} s", f64::from(r.num) / f64::from(r.denom)))
                }
            }
            _ => None,
        });

    out.f_number = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Rational(v) if !v.is_empty() => {
                let r = v[0];
                Some(format!("f/{:.1}", f64::from(r.num) / f64::from(r.denom)))
            }
            _ => None,
        });

    out.iso = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    out.focal_length = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Rational(v) if !v.is_empty() => {
                let r = v[0];
                Some(format!("{:.0} mm", f64::from(r.num) / f64::from(r.denom)))
            }
            _ => None,
        });

    out
}

/// Map EXIF orientation to rotation steps (0-3, each = 90 CW).
pub(crate) fn orientation_rotation(orientation: u32) -> u32 {
    match orientation {
        3 => 2,
        6 => 1,
        8 => 3,
        _ => 0,
    }
}

// ── Prefetch cache ──────────────────────────────────────────────────────────

struct DecodeRequest {
    path: PathBuf,
    generation: u64,
}

struct DecodeResult {
    pub path: PathBuf,
    pub image: Result<DecodedImage, String>,
    pub exif: ExifData,
    pub file_size: u64,
}

pub(crate) struct CacheEntry {
    pub image: Arc<DecodedImage>,
    pub exif: ExifData,
    pub file_size: u64,
    last_used: u64,
}

pub(crate) struct PrefetchCache {
    cache: HashMap<PathBuf, CacheEntry>,
    max_entries: usize,
    use_counter: u64,

    request_tx: mpsc::Sender<DecodeRequest>,
    result_rx: mpsc::Receiver<DecodeResult>,

    generation: Arc<AtomicU64>,
    in_flight: HashSet<PathBuf>,

    pub waiting_for_current: bool,
}

impl PrefetchCache {
    pub fn new(registry: Arc<PluginRegistry>, proxy: EventLoopProxy<AppEvent>) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<DecodeRequest>();
        let request_rx = Arc::new(Mutex::new(request_rx));
        let (result_tx, result_rx) = mpsc::channel::<DecodeResult>();
        let generation = Arc::new(AtomicU64::new(0));

        for i in 0..WORKER_COUNT {
            let rx      = Arc::clone(&request_rx);
            let tx      = result_tx.clone();
            let reg     = Arc::clone(&registry);
            let gen     = Arc::clone(&generation);
            let proxy_i = proxy.clone();
            std::thread::Builder::new()
                .name(format!("prefetch-worker-{i}"))
                .spawn(move || {
                    worker_loop(&rx, &tx, &reg, &gen, &proxy_i);
                })
                .expect("failed to spawn prefetch worker");
        }

        Self {
            cache: HashMap::new(),
            max_entries: 5,
            use_counter: 0,
            request_tx,
            result_rx,
            generation,
            in_flight: HashSet::new(),
            waiting_for_current: false,
        }
    }

    /// Try to get a decoded image from cache. Returns None on miss.
    /// The entry stays in cache (Arc clone is cheap).
    pub fn get(&mut self, path: &Path) -> Option<CacheEntry> {
        self.use_counter += 1;
        let counter = self.use_counter;
        self.cache.get_mut(path).map(|entry| {
            entry.last_used = counter;
            CacheEntry {
                image:     Arc::clone(&entry.image),
                exif:      entry.exif.clone(),
                file_size: entry.file_size,
                last_used: entry.last_used,
            }
        })
    }

    /// Request decoding of `path`. Skips if already cached or in-flight.
    pub fn request(&mut self, path: PathBuf) {
        if self.cache.contains_key(&path) || self.in_flight.contains(&path) {
            return;
        }
        let gen = self.generation.load(Ordering::Relaxed);
        self.in_flight.insert(path.clone());
        // If the worker has exited, send will fail silently — that's fine.
        let _ = self.request_tx.send(DecodeRequest {
            path,
            generation: gen,
        });
    }

    /// Drain completed decode results from the worker thread, insert all into
    /// cache, and return the paths that were successfully inserted.
    pub fn poll_into_cache(&mut self) -> Vec<PathBuf> {
        let mut inserted = Vec::new();
        while let Ok(result) = self.result_rx.try_recv() {
            self.in_flight.remove(&result.path);
            match result.image {
                Ok(image) => {
                    self.use_counter += 1;
                    let path = result.path.clone();
                    self.cache.insert(result.path, CacheEntry {
                        image: Arc::new(image),
                        exif: result.exif,
                        file_size: result.file_size,
                        last_used: self.use_counter,
                    });
                    inserted.push(path);
                }
                Err(e) => {
                    log::error!("decode {}: {e}", result.path.display());
                }
            }
        }
        if !inserted.is_empty() {
            self.evict_if_needed();
        }
        inserted
    }

    /// Remove a path from cache (e.g. after file deletion).
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.remove(path);
    }

    /// Increment generation counter, causing stale prefetch to be skipped.
    /// Clears `in_flight` so new requests for the same paths can be sent.
    pub fn bump_generation(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.in_flight.clear();
    }

    fn evict_if_needed(&mut self) {
        while self.cache.len() > self.max_entries {
            let victim = self.cache.iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            if let Some(k) = victim {
                self.cache.remove(&k);
            } else {
                break;
            }
        }
    }
}

fn worker_loop(
    rx: &Mutex<mpsc::Receiver<DecodeRequest>>,
    tx: &mpsc::Sender<DecodeResult>,
    registry: &PluginRegistry,
    generation: &AtomicU64,
    proxy: &EventLoopProxy<AppEvent>,
) {
    loop {
        // Scope the lock so we hold it only across recv(); the decode below
        // must NOT hold it, otherwise workers wouldn't actually run in parallel.
        let req = {
            let guard = rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match guard.recv() {
                Ok(r) => r,
                Err(_) => return, // sender dropped — shut down
            }
        };

        // Skip stale prefetch requests
        let current_gen = generation.load(Ordering::Relaxed);
        if req.generation < current_gen {
            log::debug!("skip stale request: {}", req.path.display());
            continue;
        }

        let t0 = std::time::Instant::now();

        let file_size = std::fs::metadata(&req.path)
            .map(|m| m.len())
            .unwrap_or(0);

        let data = match std::fs::read(&req.path) {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(DecodeResult {
                    path: req.path,
                    image: Err(e.to_string()),
                    exif: ExifData::default(),
                    file_size: 0,
                });
                let _ = proxy.send_event(AppEvent::DecodeReady);
                continue;
            }
        };

        let ext = req.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let image = match registry.find_for_extension(ext) {
            Some(plugin) => plugin.decode(&data).map(|mut img| {
                img.premultiply_alpha();
                img
            }).map_err(|e| e.to_string()),
            None => Err(format!("no plugin for '{ext}'")),
        };

        let exif = if image.is_ok() { extract_exif(&data) } else { ExifData::default() };

        log::debug!("decoded {} ({} bytes) in {:.0?}",
            req.path.display(), file_size, t0.elapsed());

        let _ = tx.send(DecodeResult {
            path: req.path,
            image,
            exif,
            file_size,
        });
        let _ = proxy.send_event(AppEvent::DecodeReady);
    }
}
