use std::path::PathBuf;

use clap::Parser;
use winit::event_loop::EventLoop;

mod app;
use app::App;

#[derive(Parser)]
#[command(name = "imageviewer", about = "A fast, cross-platform image viewer")]
struct Cli {
    /// Image file or directory to open. If omitted, opens the current directory.
    path: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    let start_path = match cli.path {
        Some(p) => Some(p.canonicalize().unwrap_or(p)),
        None => std::env::current_dir().ok(),
    };

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut App::new(start_path))?;
    Ok(())
}
