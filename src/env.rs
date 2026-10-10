use std::path::{Path, PathBuf};

/// Target operating system. Used only to select per-platform config roots so
/// that new platforms can be added without touching adapter logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// An abstract view of the ambient environment.
///
/// Every path lookup the tool performs goes through this struct instead of
/// `std::env` directly. That keeps core logic portable and makes the whole
/// tool testable against a synthetic `$HOME`/`$XDG_CONFIG_HOME`/`$PATH`
/// without mutating process-global state.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub os: Os,
}

impl Env {
    /// Snapshot the real process environment.
    pub fn from_process() -> Env {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        let path_dirs = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        Env {
            home,
            xdg_config_home,
            path_dirs,
            os: Os::current(),
        }
    }

    /// A completely synthetic environment, primarily for tests.
    pub fn synthetic(home: impl Into<PathBuf>, path_dirs: Vec<PathBuf>) -> Env {
        Env {
            home: Some(home.into()),
            xdg_config_home: None,
            path_dirs,
            os: Os::Linux,
        }
    }

    /// Per-user configuration root.
    ///
    /// Linux-first: honours `$XDG_CONFIG_HOME`, falling back to `$HOME/.config`.
    /// Other platforms are intentionally left as a single branch point so
    /// macOS (`~/Library/Application Support`) and Windows (`%APPDATA%`) can be
    /// added later without touching adapters.
    pub fn config_home(&self) -> Option<PathBuf> {
        match self.os {
            Os::Macos | Os::Windows => self.config_home_xdg(),
            Os::Linux => self.config_home_xdg(),
        }
    }

    fn config_home_xdg(&self) -> Option<PathBuf> {
        if let Some(dir) = &self.xdg_config_home {
            return Some(dir.clone());
        }
        self.home.as_ref().map(|h| h.join(".config"))
    }

    /// Expand a `~`-relative path against `$HOME`.
    pub fn expand_home(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if let Ok(rest) = path.strip_prefix("~") {
            if let Some(home) = &self.home {
                return home.join(rest);
            }
        }
        path.to_path_buf()
    }

    /// Resolve an app config path against the platform config root.
    ///
    /// Relative paths are joined to `config_home`; absolute and `~`-prefixed
    /// paths are resolved as-is. Returns `None` only when a relative path is
    /// requested but no config root exists.
    pub fn app_config(&self, relative: impl AsRef<Path>) -> Option<PathBuf> {
        let relative = relative.as_ref();
        if relative.is_absolute() || relative.starts_with("~") {
            Some(self.expand_home(relative))
        } else {
            self.config_home().map(|c| c.join(relative))
        }
    }

    /// Locate an executable on `$PATH`, mirroring `which`.
    pub fn find_in_path(&self, program: &str) -> Option<PathBuf> {
        for dir in &self.path_dirs {
            let candidate = dir.join(program);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// Whether `program` is present on `$PATH`.
    pub fn is_on_path(&self, program: &str) -> bool {
        self.find_in_path(program).is_some()
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => meta.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_wins_over_default() {
        let mut env = Env::synthetic("/home/u", vec![]);
        assert_eq!(env.config_home(), Some(PathBuf::from("/home/u/.config")));
        env.xdg_config_home = Some(PathBuf::from("/xdg"));
        assert_eq!(env.config_home(), Some(PathBuf::from("/xdg")));
    }

    #[test]
    fn expand_tilde() {
        let env = Env::synthetic("/home/u", vec![]);
        assert_eq!(
            env.expand_home("~/.tmux.conf"),
            PathBuf::from("/home/u/.tmux.conf")
        );
    }

    #[test]
    fn app_config_relative_and_absolute() {
        let env = Env::synthetic("/home/u", vec![]);
        assert_eq!(
            env.app_config("kitty/kitty.conf").unwrap(),
            PathBuf::from("/home/u/.config/kitty/kitty.conf")
        );
        assert_eq!(
            env.app_config("~/.tmux.conf").unwrap(),
            PathBuf::from("/home/u/.tmux.conf")
        );
    }

    #[test]
    fn finds_executable_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("kitty");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let env = Env::synthetic("/home/u", vec![dir.path().to_path_buf()]);
        assert!(env.is_on_path("kitty"));
        assert!(!env.is_on_path("foot"));
    }
}
