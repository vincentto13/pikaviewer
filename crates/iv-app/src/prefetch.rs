use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use iv_core::format::{DecodedImage, PluginRegistry};

// ── EXIF extraction (moved from app.rs so background worker can use it) ─────

#[derive(Default)]
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
    let exif = match reader.read_from_container(&mut cursor) {
        Ok(e)  => e,
        Err(_) => return out,
    };

    let str_field = |tag| -> Option<String> {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
    };

    out.orientation = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Short(v) if !v.is_empty() => Some(v[0] as u32),
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
                    Some(format!("{:.1} s", r.num as f64 / r.denom as f64))
                }
            }
            _ => None,
        });

    out.f_number = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Rational(v) if !v.is_empty() => {
                let r = v[0];
                Some(format!("f/{:.1}", r.num as f64 / r.denom as f64))
            }
            _ => None,
        });

    out.iso = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    out.focal_length = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Rational(v) if !v.is_empty() => {
                let r = v[0];
                Some(format!("{:.0} mm", r.num as f64 / r.denom as f64))
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

pub(crate) struct DecodeResult {
    pub path: PathBuf,
    pub image: Result<DecodedImage, String>,
    pub exif: ExifData,
    pub file_size: u64,
}

pub(crate) struct CacheEntry {
    pub image: DecodedImage,
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
    pub fn new(registry: Arc<PluginRegistry>) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<DecodeRequest>();
        let (result_tx, result_rx) = mpsc::channel::<DecodeResult>();
        let generation = Arc::new(AtomicU64::new(0));
        let gen_clone = Arc::clone(&generation);

        std::thread::Builder::new()
            .name("prefetch-worker".into())
            .spawn(move || {
                worker_loop(request_rx, result_tx, &registry, &gen_clone);
            })
            .expect("failed to spawn prefetch worker");

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
    pub fn get(&mut self, path: &Path) -> Option<CacheEntry> {
        self.cache.remove(path).map(|mut entry| {
            self.use_counter += 1;
            entry.last_used = self.use_counter;
            entry
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

    /// Non-blocking drain of completed results. Inserts into cache.
    pub fn poll(&mut self) -> Vec<DecodeResult> {
        let mut ready = Vec::new();
        while let Ok(result) = self.result_rx.try_recv() {
            self.in_flight.remove(&result.path);
            if result.image.is_ok() {
                // We'll return the result to the caller; they decide
                // whether to consume it (current image) or let it cache.
                ready.push(result);
            } else {
                log::error!("decode {}: {}", result.path.display(),
                    result.image.as_ref().err().unwrap());
            }
        }
        ready
    }

    /// Insert a decode result into cache (for prefetched images).
    pub fn insert(&mut self, result: DecodeResult) {
        if let Ok(image) = result.image {
            self.use_counter += 1;
            self.cache.insert(result.path, CacheEntry {
                image,
                exif: result.exif,
                file_size: result.file_size,
                last_used: self.use_counter,
            });
            self.evict_if_needed();
        }
    }

    /// Remove a path from cache (e.g. after file deletion).
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.remove(path);
    }

    /// Increment generation counter, causing stale prefetch to be skipped.
    pub fn bump_generation(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
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
    rx: mpsc::Receiver<DecodeRequest>,
    tx: mpsc::Sender<DecodeResult>,
    registry: &PluginRegistry,
    generation: &AtomicU64,
) {
    while let Ok(req) = rx.recv() {
        // Skip stale prefetch requests
        let current_gen = generation.load(Ordering::Relaxed);
        if req.generation < current_gen {
            continue;
        }

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
                continue;
            }
        };

        let ext = req.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let image = match registry.find_for_extension(ext) {
            Some(plugin) => plugin.decode(&data).map_err(|e| e.to_string()),
            None => Err(format!("no plugin for '{ext}'")),
        };

        let exif = if image.is_ok() { extract_exif(&data) } else { ExifData::default() };

        let _ = tx.send(DecodeResult {
            path: req.path,
            image,
            exif,
            file_size,
        });
    }
}
