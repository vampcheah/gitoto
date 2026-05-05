mod action;
mod app;
mod components;
mod config;
mod event;
mod git;
mod repo_id;
mod tui;
mod update_checker;
mod watcher;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "gitoto", about = "Multi-repo Git source control panel")]
struct Cli {
    /// Root directory to scan for repos (defaults to the current directory)
    #[arg(long)]
    root: Option<PathBuf>,

    /// UI frame rate (deprecated — rendering is now on-demand)
    #[arg(long, hide = true)]
    frame_rate: Option<u16>,

    /// Start in fast mode: skip automatic fetch polling, graph stats, and untracked scans
    #[arg(long)]
    fast: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Update gitoto to the latest published version via cargo install
    Update,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    if let Some(Command::Update) = cli.command {
        return self_update();
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("gitoto=info".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut config = config::Config::load()?;

    config.override_root(root_dir(cli.root)?);
    if cli.fast {
        config.performance.fast_mode = true;
    }
    if let Some(frame_rate) = cli.frame_rate {
        config.ui.frame_rate = frame_rate;
    }

    let mut app = app::App::new(config);
    app.run().await?;

    Ok(())
}

fn root_dir(cli_root: Option<PathBuf>) -> Result<PathBuf> {
    Ok(match cli_root {
        Some(root) => root,
        None => std::env::current_dir()?,
    })
}

fn self_update() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("gitoto v{current} — checking for updates...");

    if let Some(latest) = update_checker::check_latest() {
        println!("New version available: v{latest}");
    } else {
        println!("Already up to date.");
        return Ok(());
    }

    println!("Running: cargo install gitoto");
    let status = std::process::Command::new("cargo")
        .args(["install", "gitoto"])
        .status();

    match status {
        Ok(s) if s.success() => println!("Updated successfully."),
        Ok(s) => {
            eprintln!("cargo install exited with {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run cargo: {e}");
            eprintln!("Make sure cargo is installed (https://rustup.rs)");
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_dir_is_current_dir() {
        assert_eq!(root_dir(None).unwrap(), std::env::current_dir().unwrap());
    }

    #[test]
    fn cli_root_overrides_current_dir() {
        let root = PathBuf::from("/tmp/my-repos");
        assert_eq!(root_dir(Some(root.clone())).unwrap(), root);
    }
}
