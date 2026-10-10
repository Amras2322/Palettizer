use crate::env::Env;
use std::path::PathBuf;

/// Outcome of probing whether an app can receive a palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectStatus {
    /// Binary on `$PATH` and at least one config file/dir exists.
    Compatible,
    /// Binary not found on `$PATH`.
    NotInstalled,
    /// Binary present but no config file/dir was found.
    ConfigMissing,
    /// Binary and config present, but the config format is not one this adapter
    /// can safely patch (for example a legacy format).
    Unsupported,
}

impl DetectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectStatus::Compatible => "compatible",
            DetectStatus::NotInstalled => "not installed",
            DetectStatus::ConfigMissing => "config missing",
            DetectStatus::Unsupported => "unsupported",
        }
    }
}

/// Full result of detecting one app.
#[derive(Debug, Clone)]
pub struct DetectResult {
    pub status: DetectStatus,
    /// Resolved path of the binary, when found.
    pub binary: Option<PathBuf>,
    /// Resolved path of the config file that would be patched.
    pub config_path: Option<PathBuf>,
    /// Human-readable explanation, used for skips and `list`/`status`.
    pub reason: Option<String>,
}

impl DetectResult {
    pub fn compatible(binary: PathBuf, config: PathBuf) -> Self {
        DetectResult {
            status: DetectStatus::Compatible,
            binary: Some(binary),
            config_path: Some(config),
            reason: None,
        }
    }

    pub fn not_installed(app: impl AsRef<str>) -> Self {
        DetectResult {
            reason: Some(format!("`{}` not found on $PATH", app.as_ref())),
            status: DetectStatus::NotInstalled,
            binary: None,
            config_path: None,
        }
    }

    pub fn config_missing(binary: PathBuf, reason: String) -> Self {
        DetectResult {
            status: DetectStatus::ConfigMissing,
            binary: Some(binary),
            config_path: None,
            reason: Some(reason),
        }
    }

    pub fn unsupported(binary: PathBuf, path: PathBuf, reason: String) -> Self {
        DetectResult {
            status: DetectStatus::Unsupported,
            binary: Some(binary),
            config_path: Some(path),
            reason: Some(reason),
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.status == DetectStatus::Compatible
    }
}

/// Convenience: detect using a binary name and a set of candidate config paths.
///
/// The first existing candidate becomes the reported config path. `app` is used
/// for the error strings so adapters with multiple config files can share this.
pub fn detect_with(
    env: &Env,
    app: &str,
    binary_name: &str,
    candidates: &[PathBuf],
) -> DetectResult {
    let Some(binary) = env.find_in_path(binary_name) else {
        return DetectResult::not_installed(app);
    };
    match candidates.iter().find(|p| p.exists()) {
        Some(found) => DetectResult::compatible(binary, found.clone()),
        None => DetectResult::config_missing(
            binary,
            format!("`{binary_name}` found, but none of the candidate config paths exist"),
        ),
    }
}
