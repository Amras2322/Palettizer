use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use palettizer::detect::DetectStatus;
use palettizer::engine::{self, AppReport, ApplyOptions};
use palettizer::env::Env;
use palettizer::palette::Palette;
use palettizer::{adapters, backup};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "palettizer",
    version,
    about = "Sync a single source-of-truth color palette to compatible TUI apps",
    long_about = "Palettizer reads a YAML palette and patches the color keys of \
                  supported terminal apps, backing up each file before it changes.",
    after_help = "Examples:\n  palettizer apply\n  palettizer apply kitty --diff\n  palettizer status\n  palettizer rollback btop"
)]
struct Cli {
    /// Palette YAML path (default: ~/.config/palettizer/palette.yaml).
    #[arg(long, global = true, env = "PALETTIZER_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Apply the palette to every compatible app (or the listed apps).
    Apply {
        /// Restrict to these apps (default: all detected apps).
        apps: Vec<String>,
        /// Show what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Skip the pre-apply backup.
        #[arg(long)]
        no_backup: bool,
        /// Print a unified diff of each changed file.
        #[arg(long)]
        diff: bool,
    },
    /// List detected apps and whether a palette can be applied.
    List,
    /// Show whether the palette is currently applied, per app.
    Status {
        /// Restrict to these apps (default: all).
        apps: Vec<String>,
    },
    /// Restore the most recent backup for each app.
    Rollback {
        /// Restrict to these apps (default: all).
        apps: Vec<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let env = Env::from_process();
    if let Err(e) = run(cli, &env) {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

fn run(cli: Cli, env: &Env) -> Result<()> {
    match cli.command {
        Command::Apply {
            apps,
            dry_run,
            no_backup,
            diff,
        } => {
            let palette = load_palette(env, cli.config.as_deref())?;
            let opts = ApplyOptions {
                dry_run,
                no_backup,
                diff,
            };
            let reports = engine::run_apply(env, &palette, &apps, &opts)?;
            let failed = print_apply(&reports, dry_run);
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Status { apps } => {
            let palette = load_palette(env, cli.config.as_deref())?;
            let reports = engine::run_status(env, &palette, &apps)?;
            print_status(&reports);
            Ok(())
        }
        Command::List => {
            let palette = load_palette(env, cli.config.as_deref()).ok();
            print_list(env, palette.as_ref());
            Ok(())
        }
        Command::Rollback { apps } => {
            let reports = engine::run_rollback(env, &apps)?;
            let mut failed = 0;
            for r in &reports {
                match &r.result {
                    Ok(backup::RollbackOutcome::NoBackup) => {
                        println!("{}: no backup found", r.app);
                    }
                    Ok(backup::RollbackOutcome::Restored { timestamp, actions }) => {
                        println!(
                            "{}: restored {} file(s) from backup {timestamp}",
                            r.app,
                            actions.len()
                        );
                        for action in actions {
                            match action {
                                backup::RollbackAction::Restored(p) => {
                                    println!("  restored {}", p.display())
                                }
                                backup::RollbackAction::Removed(p) => {
                                    println!("  removed  {}", p.display())
                                }
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        println!("{}: rollback failed: {e:#}", r.app);
                    }
                }
            }
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn default_palette_path(env: &Env) -> Option<PathBuf> {
    env.app_config("palettizer/palette.yaml")
}

fn load_palette(env: &Env, explicit: Option<&std::path::Path>) -> Result<Palette> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => default_palette_path(env)
            .ok_or_else(|| anyhow::anyhow!("cannot resolve palette path (no HOME/config dir)"))?,
    };
    if !path.exists() {
        bail!(
            "palette file not found: {}\nCreate one or pass --config <path>.",
            path.display()
        );
    }
    Palette::load(&path)
}

fn print_apply(reports: &[AppReport], dry_run: bool) -> usize {
    let mut failed = 0;
    let mut changed_files = 0;
    let mut applied_apps = 0;

    for r in reports {
        if let Some(err) = &r.error {
            failed += 1;
            println!("{}: FAILED - {err}", r.app);
            continue;
        }
        if !r.detect.is_compatible() {
            continue;
        }
        let changed: Vec<_> = r.files.iter().filter(|f| f.changed).collect();
        if changed.is_empty() {
            println!("{}: already up to date", r.app);
            continue;
        }
        applied_apps += 1;
        changed_files += changed.len();
        let verb = if dry_run { "would update" } else { "updated" };
        println!(
            "{}: {} {} file(s) [{}]",
            r.app,
            verb,
            changed.len(),
            r.keys_touched.join(", ")
        );
        for f in changed {
            let suffix = match &f.backup {
                Some(b) => format!("  (backup: {})", b.display()),
                None if !dry_run => "  (no backup)".to_string(),
                None => String::new(),
            };
            println!("  {}{}", f.path.display(), suffix);
            if let Some(diff) = &f.diff {
                print!("{diff}");
            }
        }
    }

    if dry_run {
        println!("dry run: {applied_apps} app(s), {changed_files} file(s) would change");
    } else {
        println!("applied palette to {applied_apps} app(s), {changed_files} file(s)");
    }
    failed
}

fn print_status(reports: &[AppReport]) {
    for r in reports {
        if let Some(err) = &r.error {
            println!("{:<10} error       {}", r.app, err);
            continue;
        }
        match r.detect.status {
            DetectStatus::Compatible => {
                let state = if r.files.iter().any(|f| f.changed) {
                    "needs apply"
                } else {
                    "up to date"
                };
                let path = r
                    .detect
                    .config_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                println!("{:<10} {:<12} {}", r.app, state, path);
                if !r.keys_touched.is_empty() {
                    println!("           keys: {}", r.keys_touched.join(", "));
                }
                for f in r.files.iter().filter(|f| f.changed) {
                    println!("           pending: {}", f.path.display());
                }
            }
            other => {
                let reason = r.detect.reason.clone().unwrap_or_default();
                println!("{:<10} {:<12} {}", r.app, other.as_str(), reason);
            }
        }
    }
}

fn print_list(env: &Env, palette: Option<&Palette>) {
    for adapter in adapters::registry() {
        let detect = adapter.detect(env);
        let path = detect
            .config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        let keys = if detect.is_compatible() {
            palette
                .and_then(|p| adapter.patch(env, p).ok())
                .map(|patched| format!("{} keys", patched.keys_touched.len()))
                .unwrap_or_else(|| "palette unavailable".to_string())
        } else {
            detect.reason.clone().unwrap_or_default()
        };
        let binary = detect
            .binary
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<10} {:<12} {}  bin:{}  {}",
            adapter.name(),
            detect.status.as_str(),
            path,
            binary,
            keys
        );
    }
}
