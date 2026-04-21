use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use iv_core::format::PluginRegistry;
use iv_formats::default_registry;
use winit::event_loop::EventLoop;
use app::AppEvent;

mod app;
mod config;
mod desktop_integration;
mod dir_watcher;
mod prefetch;
mod settings_window;
use app::App;

#[cfg(target_os = "macos")]
mod macos_events;

#[derive(Parser)]
#[command(name = "pikaviewer", about = "A fast, cross-platform image viewer")]
struct Cli {
    /// Image file or directory to open. If omitted, opens the current directory.
    path: Option<PathBuf>,

    /// Install .desktop file and set as default image viewer (Linux only).
    #[arg(long)]
    install: bool,

    /// Remove .desktop file (Linux only).
    #[arg(long)]
    uninstall: bool,
}

fn build_registry() -> PluginRegistry {
    #[allow(unused_mut)]
    let mut r = default_registry();
    #[cfg(feature = "heic")]
    r.register(iv_format_heic::HeicPlugin);
    #[cfg(feature = "raw")]
    r.register(iv_format_raw::RawPlugin);
    r
}

fn init_logging() {
    use std::io::Write;

    if cfg!(debug_assertions) {
        // Debug builds: log to /tmp/pikaviewer.log at debug level
        let log_path = std::path::Path::new("/tmp/pikaviewer.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path);

        match file {
            Ok(file) => {
                let file = std::sync::Mutex::new(file);
                env_logger::Builder::new()
                    .filter_level(log::LevelFilter::Debug)
                    .filter_module("wgpu", log::LevelFilter::Warn)
                    .filter_module("naga", log::LevelFilter::Warn)
                    .format(move |buf, record| {
                        let ts = buf.timestamp_millis();
                        let line = format!(
                            "{ts} [{:5}] {}: {}\n",
                            record.level(),
                            record.target(),
                            record.args()
                        );
                        // Write to both stderr and log file
                        let _ = buf.write_all(line.as_bytes());
                        if let Ok(mut f) = file.lock() {
                            let _ = f.write_all(line.as_bytes());
                            let _ = f.flush();
                        }
                        Ok(())
                    })
                    .init();
                log::info!("logging to {}", log_path.display());
                return;
            }
            Err(e) => {
                eprintln!("warning: could not open {}: {e}", log_path.display());
            }
        }
    }

    // Release builds (or fallback): stderr only, controlled by RUST_LOG
    env_logger::init();
}

fn main() -> anyhow::Result<()> {
    init_logging();

    // macOS passes -psn_XXXXXXXX (Process Serial Number) when the app is
    // launched by Finder / Launch Services. Strip it before clap sees it,
    // otherwise clap exits with "unexpected argument".
    let args: Vec<_> = std::env::args_os()
        .filter(|a| !a.to_string_lossy().starts_with("-psn_"))
        .collect();

    let cli = Cli::parse_from(args);

    // Handle --install / --uninstall (Linux only, exits immediately)
    #[cfg(target_os = "linux")]
    {
        if cli.install {
            return desktop_integration::install();
        }
        if cli.uninstall {
            return desktop_integration::uninstall();
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if cli.install || cli.uninstall {
            anyhow::bail!("--install and --uninstall are only supported on Linux");
        }
    }

    let start_path = match cli.path {
        Some(p) => Some(p),
        None => std::env::current_dir().ok(),
    };

    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    // macOS: inject application:openURLs: into the delegate class NOW (if the
    // delegate is set already) AND register a WillFinishLaunching notification
    // observer as a fallback. Both run before finishLaunching processes queued
    // Apple Events, which is the critical window.
    #[cfg(target_os = "macos")]
    macos_events::register();

    event_loop.run_app(&mut App::new(start_path, Arc::new(build_registry()), proxy))?;
    Ok(())
}
