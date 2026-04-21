use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use winit::event_loop::EventLoopProxy;

use iv_core::format::PluginRegistry;

use crate::app::AppEvent;

/// How long to coalesce filesystem events before notifying the app.
/// Editors and file managers often emit bursts of create/rename/remove
/// events for a single user-visible operation; 500 ms absorbs them into
/// a single rescan.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Watches a single directory and fires `AppEvent::DirChanged` on the event
/// loop when a supported image file is added, removed or renamed. The
/// underlying `notify` watcher is held alive for the lifetime of this struct;
/// dropping it stops the debounce thread as well.
pub(crate) struct DirWatcher {
    _watcher: RecommendedWatcher,
    stop_tx:  mpsc::Sender<()>,
    dir:      PathBuf,
}

impl DirWatcher {
    pub fn new(
        dir: &Path,
        registry: &Arc<PluginRegistry>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> notify::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel::<notify::Event>();
        let (stop_tx,  stop_rx)  = mpsc::channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        })?;
        watcher.watch(dir, RecursiveMode::NonRecursive)?;

        let supported: Vec<String> = registry
            .supported_extensions()
            .iter()
            .map(|s| (*s).to_ascii_lowercase())
            .collect();

        std::thread::Builder::new()
            .name("dir-watcher-debounce".into())
            .spawn(move || debounce_loop(&event_rx, &stop_rx, &supported, &proxy))
            .expect("failed to spawn dir-watcher thread");

        Ok(Self { _watcher: watcher, stop_tx, dir: dir.to_path_buf() })
    }

    pub fn dir(&self) -> &Path { &self.dir }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

fn debounce_loop(
    event_rx: &mpsc::Receiver<notify::Event>,
    stop_rx:  &mpsc::Receiver<()>,
    supported: &[String],
    proxy:    &EventLoopProxy<AppEvent>,
) {
    let mut pending_since: Option<Instant> = None;

    loop {
        if stop_rx.try_recv().is_ok() { return; }

        match event_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                if is_relevant(&event, supported) {
                    pending_since = Some(Instant::now());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if let Some(t) = pending_since {
            if t.elapsed() >= DEBOUNCE {
                pending_since = None;
                let _ = proxy.send_event(AppEvent::DirChanged);
            }
        }
    }
}

fn is_relevant(event: &notify::Event, supported: &[String]) -> bool {
    // Only react to events that can change the directory listing. Modify
    // events on existing files don't affect the list of supported images.
    let structural = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
    );
    if !structural { return false; }

    event.paths.iter().any(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                let ext = ext.to_ascii_lowercase();
                supported.iter().any(|s| s == &ext)
            })
    })
}
