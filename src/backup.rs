use crate::env::Env;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// One backed-up (or created) file from an apply run.
///
/// `backup` is `None` when the target file did not exist before palettizer ran
/// (for example a freshly generated theme); rollback deletes such files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupEntry {
    pub original: PathBuf,
    pub backup: Option<PathBuf>,
}

/// Manifest describing everything an apply run overwrote, stored next to the
/// timestamped `.bak` files and pointed to by `latest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub app: String,
    pub timestamp: String,
    pub entries: Vec<BackupEntry>,
}

/// Backup store rooted at `<config>/.palettizer/backups`.
pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new(env: &Env) -> Result<BackupStore> {
        let root = env
            .config_home()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve config home for backups"))?
            .join(".palettizer")
            .join("backups");
        Ok(BackupStore { root })
    }

    pub fn from_root(root: PathBuf) -> BackupStore {
        BackupStore { root }
    }

    pub fn app_dir(&self, app: &str) -> PathBuf {
        self.root.join(app)
    }

    /// Copy every existing file in `planned` into the app's backup dir and
    /// return a manifest. Files are backed up with a `<name>.<timestamp>.bak`
    /// suffix; the manifest is written to both `latest.json` and
    /// `<timestamp>.json` so `rollback` can find the newest run reliably.
    pub fn record(
        &self,
        app: &str,
        timestamp: &str,
        planned: &[(PathBuf, bool)],
    ) -> Result<(Manifest, Vec<Option<PathBuf>>)> {
        let dir = self.app_dir(app);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating backup dir {}", dir.display()))?;

        let mut entries = Vec::new();
        let mut backups = Vec::new();
        for (path, existed) in planned {
            if *existed {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "config".to_string());
                let backup = dir.join(format!("{name}.{timestamp}.bak"));
                std::fs::copy(path, &backup).with_context(|| {
                    format!("backing up {} -> {}", path.display(), backup.display())
                })?;
                entries.push(BackupEntry {
                    original: path.clone(),
                    backup: Some(backup.clone()),
                });
                backups.push(Some(backup));
            } else {
                entries.push(BackupEntry {
                    original: path.clone(),
                    backup: None,
                });
                backups.push(None);
            }
        }

        let manifest = Manifest {
            app: app.to_string(),
            timestamp: timestamp.to_string(),
            entries,
        };
        let json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(dir.join(format!("{timestamp}.json")), &json)?;
        std::fs::write(dir.join("latest.json"), &json)?;
        Ok((manifest, backups))
    }

    pub fn latest(&self, app: &str) -> Result<Option<Manifest>> {
        let path = self.app_dir(app).join("latest.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let manifest = serde_json::from_str(&text)
            .with_context(|| format!("parsing manifest {}", path.display()))?;
        Ok(Some(manifest))
    }

    /// Restore the most recent manifest for `app`.
    pub fn rollback(&self, app: &str) -> Result<RollbackOutcome> {
        let Some(manifest) = self.latest(app)? else {
            return Ok(RollbackOutcome::NoBackup);
        };
        let mut restored = Vec::new();
        for entry in &manifest.entries {
            match &entry.backup {
                Some(backup) => {
                    std::fs::copy(backup, &entry.original).with_context(|| {
                        format!(
                            "restoring {} -> {}",
                            backup.display(),
                            entry.original.display()
                        )
                    })?;
                    restored.push(RollbackAction::Restored(entry.original.clone()));
                }
                None => {
                    if entry.original.exists() {
                        std::fs::remove_file(&entry.original).ok();
                        restored.push(RollbackAction::Removed(entry.original.clone()));
                    }
                }
            }
        }
        Ok(RollbackOutcome::Restored {
            timestamp: manifest.timestamp,
            actions: restored,
        })
    }
}

#[derive(Debug)]
pub enum RollbackOutcome {
    NoBackup,
    Restored {
        timestamp: String,
        actions: Vec<RollbackAction>,
    },
}

#[derive(Debug)]
pub enum RollbackAction {
    Restored(PathBuf),
    Removed(PathBuf),
}

/// A filesystem-safe UTC timestamp: `YYYYMMDD-HHMMSSZ`.
pub fn timestamp_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}{mo:02}{d:02}-{hh:02}{mm:02}{ss:02}Z")
}

/// Howard Hinnant's days-from-civil inverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_shape() {
        // 2021-01-01T00:00:00Z == 1609459200
        assert_eq!(format_utc(1_609_459_200), "20210101-000000Z");
        // 1970-01-01T00:00:00Z
        assert_eq!(format_utc(0), "19700101-000000Z");
    }

    #[test]
    fn backup_and_rollback_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = BackupStore::from_root(dir.path().join("backups"));
        let original = dir.path().join("kitty.conf");
        std::fs::write(&original, "original\n").unwrap();

        let (manifest, _) = store
            .record("kitty", "20260101-000000Z", &[(original.clone(), true)])
            .unwrap();
        assert_eq!(manifest.entries.len(), 1);
        std::fs::write(&original, "patched\n").unwrap();

        let outcome = store.rollback("kitty").unwrap();
        match outcome {
            RollbackOutcome::Restored { actions, .. } => {
                assert_eq!(actions.len(), 1);
                assert_eq!(std::fs::read_to_string(&original).unwrap(), "original\n");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rollback_removes_created_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = BackupStore::from_root(dir.path().join("backups"));
        let created = dir.path().join("palettizer.theme");
        std::fs::write(&created, "generated\n").unwrap();
        store
            .record("btop", "20260101-000000Z", &[(created.clone(), false)])
            .unwrap();
        store.rollback("btop").unwrap();
        assert!(!created.exists());
    }

    #[test]
    fn rollback_without_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = BackupStore::from_root(dir.path().join("backups"));
        assert!(matches!(
            store.rollback("nope").unwrap(),
            RollbackOutcome::NoBackup
        ));
    }
}
