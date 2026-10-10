use crate::adapters::registry;
use crate::backup::{timestamp_utc, BackupStore, RollbackOutcome};
use crate::detect::DetectResult;
use crate::env::Env;
use crate::palette::Palette;
use anyhow::Result;
use similar::TextDiff;
use std::path::{Path, PathBuf};

/// Options controlling `apply`.
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    pub dry_run: bool,
    pub no_backup: bool,
    pub diff: bool,
}

/// Per-file outcome of an apply (or of a dry-run/status probe).
#[derive(Debug, Clone)]
pub struct FileReport {
    pub path: PathBuf,
    /// The adapter's output differs from what is on disk.
    pub changed: bool,
    /// Where the pre-apply copy was stored, if a backup was taken.
    pub backup: Option<PathBuf>,
    pub diff: Option<String>,
}

/// Per-app outcome shared by `apply` and `status`.
#[derive(Debug, Clone)]
pub struct AppReport {
    pub app: String,
    pub detect: DetectResult,
    pub keys_touched: Vec<String>,
    pub files: Vec<FileReport>,
    pub error: Option<String>,
}

impl AppReport {
    /// True when the adapter succeeded and produced no changes.
    pub fn is_up_to_date(&self) -> bool {
        self.error.is_none() && self.detect.is_compatible() && !self.files.iter().any(|f| f.changed)
    }
}

fn unified_diff(old: &str, new: &str, label: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{label}"), &format!("b/{label}"))
        .to_string()
}

/// Read a file's current contents (or `""` if it does not exist) and report
/// whether the adapter's `content` would change it. Shared by `run_apply` and
/// `run_status` so `status`/`--dry-run` cannot drift from a real apply.
fn change_state(path: &Path, content: &str) -> (bool, String) {
    let old = if path.is_file() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    (old != content, old)
}

fn filter<'a>(only: &'a [String]) -> impl Iterator<Item = Box<dyn crate::adapters::Adapter>> + 'a {
    registry()
        .into_iter()
        .filter(move |a| only.is_empty() || only.iter().any(|n| n == a.name()))
}

/// Apply the palette to all (or selected) compatible apps.
///
/// A failure in one adapter never aborts the run: it is recorded on that app's
/// report and the loop continues.
pub fn run_apply(
    env: &Env,
    palette: &Palette,
    only: &[String],
    opts: &ApplyOptions,
) -> Result<Vec<AppReport>> {
    let store = BackupStore::new(env).ok();
    let timestamp = timestamp_utc();
    let mut reports = Vec::new();

    for adapter in filter(only) {
        let app = adapter.name().to_string();
        let detect = adapter.detect(env);
        if !detect.is_compatible() {
            reports.push(AppReport {
                app,
                detect,
                keys_touched: Vec::new(),
                files: Vec::new(),
                error: None,
            });
            continue;
        }

        let patched = match adapter.patch(env, palette) {
            Ok(p) => p,
            Err(e) => {
                reports.push(AppReport {
                    app,
                    detect,
                    keys_touched: Vec::new(),
                    files: Vec::new(),
                    error: Some(format!("{e:#}")),
                });
                continue;
            }
        };

        // Work out which files actually change.
        struct Planned {
            path: PathBuf,
            content: String,
            old: String,
            existed: bool,
        }
        let mut planned = Vec::new();
        for file in &patched.files {
            let (changed, old) = change_state(&file.path, &file.content);
            if changed {
                planned.push(Planned {
                    path: file.path.clone(),
                    content: file.content.clone(),
                    old,
                    existed: file.path.is_file(),
                });
            }
        }

        // Back up changed files (source of truth for rollback) before writing.
        let mut backups: Vec<Option<PathBuf>> = Vec::new();
        if !opts.dry_run && !opts.no_backup && !planned.is_empty() {
            if let Some(store) = &store {
                let inputs: Vec<(PathBuf, bool)> = planned
                    .iter()
                    .map(|p| (p.path.clone(), p.existed))
                    .collect();
                match store.record(&app, &timestamp, &inputs) {
                    Ok((_, b)) => backups = b,
                    Err(e) => {
                        reports.push(AppReport {
                            app,
                            detect,
                            keys_touched: patched.keys_touched,
                            files: Vec::new(),
                            error: Some(format!("backup failed: {e:#}")),
                        });
                        continue;
                    }
                }
            }
        }

        // Write.
        let mut file_reports = Vec::new();
        let mut write_error = None;
        for (idx, p) in planned.iter().enumerate() {
            if !opts.dry_run {
                if let Some(parent) = p.path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        write_error = Some(format!("creating {}: {e}", parent.display()));
                        break;
                    }
                }
                if let Err(e) = std::fs::write(&p.path, &p.content) {
                    write_error = Some(format!("writing {}: {e}", p.path.display()));
                    break;
                }
            }
            let label = p.path.display().to_string();
            file_reports.push(FileReport {
                path: p.path.clone(),
                changed: true,
                backup: backups.get(idx).cloned().flatten(),
                diff: if opts.diff {
                    Some(unified_diff(&p.old, &p.content, &label))
                } else {
                    None
                },
            });
        }

        reports.push(AppReport {
            app,
            detect,
            keys_touched: patched.keys_touched,
            files: file_reports,
            error: write_error,
        });
    }

    Ok(reports)
}

/// Report what `apply` would change, without writing anything.
pub fn run_status(env: &Env, palette: &Palette, only: &[String]) -> Result<Vec<AppReport>> {
    let mut reports = Vec::new();
    for adapter in filter(only) {
        let app = adapter.name().to_string();
        let detect = adapter.detect(env);
        if !detect.is_compatible() {
            reports.push(AppReport {
                app,
                detect,
                keys_touched: Vec::new(),
                files: Vec::new(),
                error: None,
            });
            continue;
        }
        match adapter.patch(env, palette) {
            Ok(patched) => {
                let files = patched
                    .files
                    .iter()
                    .map(|f| {
                        let (changed, _) = change_state(&f.path, &f.content);
                        FileReport {
                            path: f.path.clone(),
                            changed,
                            backup: None,
                            diff: None,
                        }
                    })
                    .collect();
                reports.push(AppReport {
                    app,
                    detect,
                    keys_touched: patched.keys_touched,
                    files,
                    error: None,
                });
            }
            Err(e) => reports.push(AppReport {
                app,
                detect,
                keys_touched: Vec::new(),
                files: Vec::new(),
                error: Some(format!("{e:#}")),
            }),
        }
    }
    Ok(reports)
}

/// Result of rolling back one app.
#[derive(Debug)]
pub struct RollbackReport {
    pub app: String,
    pub result: Result<RollbackOutcome>,
}

pub fn run_rollback(env: &Env, only: &[String]) -> Result<Vec<RollbackReport>> {
    let store = BackupStore::new(env)?;
    let mut reports = Vec::new();
    for adapter in filter(only) {
        let app = adapter.name().to_string();
        let result = store.rollback(&app).map_err(|e| anyhow::anyhow!("{e:#}"));
        reports.push(RollbackReport { app, result });
    }
    Ok(reports)
}
