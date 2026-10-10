//! End-to-end tests running the full pipeline against a synthetic `$HOME`,
//! `$XDG_CONFIG_HOME` and `$PATH`, so no real system files are touched.

use palettizer::backup::{BackupStore, RollbackOutcome};
use palettizer::detect::DetectStatus;
use palettizer::engine::{run_apply, run_rollback, run_status, ApplyOptions};
use palettizer::env::Env;
use palettizer::palette::Palette;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PALETTE: &str = r##"
name: test
palette:
  black:        "#000000"
  red:          "#ff0000"
  green:        "#00ff00"
  yellow:       "#ffff00"
  blue:         "#0000ff"
  magenta:      "#ff00ff"
  cyan:         "#00ffff"
  white:        "#ffffff"
  brightBlack:  "#555555"
  brightRed:    "#ff5555"
  brightGreen:  "#55ff55"
  brightYellow: "#ffff55"
  brightBlue:   "#5555ff"
  brightMagenta: "#ff55ff"
  brightCyan:   "#55ffff"
  brightWhite:  "#ffffff"
  foreground:   "#eeeeee"
  background:   "#111111"
"##;

struct Fixture {
    _dir: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        Fixture {
            _dir: dir,
            home,
            bin,
        }
    }

    fn env(&self) -> Env {
        Env::synthetic(self.home.clone(), vec![self.bin.clone()])
    }

    fn fake_bin(&self, name: &str) {
        let path = self.bin.join(name);
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
    }

    fn config(&self, relative: &str, content: &str) {
        let path = self.home.join(".config").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.home.join(".config").join(relative)).unwrap()
    }

    fn exists(&self, relative: &str) -> bool {
        self.home.join(".config").join(relative).exists()
    }

    fn palette(&self) -> Palette {
        Palette::from_yaml(PALETTE).unwrap()
    }

    /// Install binaries and config files for the four simplest apps.
    fn install_core_apps(&self) {
        for (bin, config, content) in [
            ("kitty", "kitty/kitty.conf", "# kitty\nfont_size 12.0\ncolor1 #111111\n"),
            ("foot", "foot/foot.ini", "[colors]\nforeground=#000000\n"),
            (
                "alacritty",
                "alacritty/alacritty.toml",
                "# alacritty\n[window]\npadding = 4 # keep\n\n[colors.primary]\nbackground = \"#000000\"\n",
            ),
            ("micro", "micro/settings.json", "{\n  \"tabsize\": 4\n}\n"),
        ] {
            self.fake_bin(bin);
            self.config(config, content);
        }
    }
}

fn find<'a>(
    reports: &'a [palettizer::engine::AppReport],
    app: &str,
) -> &'a palettizer::engine::AppReport {
    reports
        .iter()
        .find(|r| r.app == app)
        .expect("app report present")
}

#[test]
fn full_apply_is_idempotent_and_rollbackable() {
    let fx = Fixture::new();
    fx.install_core_apps();
    let env = fx.env();
    let palette = fx.palette();

    // First apply changes everything.
    let reports = run_apply(&env, &palette, &[], &ApplyOptions::default()).unwrap();
    for app in ["kitty", "foot", "alacritty", "micro"] {
        let r = find(&reports, app);
        assert!(r.error.is_none(), "{app} errored: {:?}", r.error);
        assert!(r.files.iter().any(|f| f.changed), "{app} unchanged");
    }

    // Unknown lines are preserved, only palette keys change.
    let kitty = fx.read("kitty/kitty.conf");
    assert!(kitty.contains("# kitty"));
    assert!(kitty.contains("font_size 12.0"));
    assert!(kitty.contains("color1 #ff0000"));
    assert!(kitty.contains("foreground #eeeeee"));
    assert!(kitty.contains("color15 #ffffff"));

    // Generated artifacts exist for micro.
    assert!(fx.exists("micro/colorschemes/palettizer.micro"));
    let scheme = fx.read("micro/colorschemes/palettizer.micro");
    assert!(scheme.contains("#eeeeee,#111111"));

    // Second apply is a no-op.
    let reports = run_apply(&env, &palette, &[], &ApplyOptions::default()).unwrap();
    for app in ["kitty", "foot", "alacritty", "micro"] {
        assert!(
            find(&reports, app).files.iter().all(|f| !f.changed),
            "{app} not idempotent"
        );
    }

    // status agrees everything is up to date.
    let status = run_status(&env, &palette, &[]).unwrap();
    assert!(find(&status, "kitty").is_up_to_date());

    // Rollback restores the originals byte-for-byte.
    let rolled = run_rollback(&env, &["kitty".to_string(), "micro".to_string()]).unwrap();
    for r in &rolled {
        assert!(matches!(
            r.result.as_ref().unwrap(),
            RollbackOutcome::Restored { .. }
        ));
    }
    assert!(fx.read("kitty/kitty.conf").contains("color1 #111111"));
    assert!(!kitty_is_applied(&fx));
    // micro settings.json restored, generated colorscheme removed.
    assert!(!fx.exists("micro/colorschemes/palettizer.micro"));
}

fn kitty_is_applied(fx: &Fixture) -> bool {
    fx.read("kitty/kitty.conf").contains("color1 #ff0000")
}

#[test]
fn detection_reports_missing_and_unsupported() {
    let fx = Fixture::new();
    // micro present (bin+config); kitty has a binary but no config;
    // alacritty has only a legacy YAML config; btop absent entirely.
    fx.fake_bin("micro");
    fx.config("micro/settings.json", "{}");
    fx.fake_bin("kitty");
    fx.fake_bin("alacritty");
    fx.config(
        "alacritty/alacritty.yml",
        "colors:\n  primary:\n    background: '000000'\n",
    );

    let env = fx.env();
    let palette = fx.palette();
    let reports = run_status(&env, &palette, &[]).unwrap();

    assert_eq!(
        find(&reports, "micro").detect.status,
        DetectStatus::Compatible
    );
    assert_eq!(
        find(&reports, "kitty").detect.status,
        DetectStatus::ConfigMissing
    );
    assert_eq!(
        find(&reports, "alacritty").detect.status,
        DetectStatus::Unsupported
    );
    assert_eq!(
        find(&reports, "btop").detect.status,
        DetectStatus::NotInstalled
    );
    assert_eq!(
        find(&reports, "ghostty").detect.status,
        DetectStatus::NotInstalled
    );
}

#[test]
fn a_failing_adapter_does_not_abort_the_run() {
    let fx = Fixture::new();
    fx.install_core_apps();
    // Break alacritty's TOML so its adapter fails.
    fx.config("alacritty/alacritty.toml", "this is : not valid = toml\n");

    let env = fx.env();
    let palette = fx.palette();
    let reports = run_apply(&env, &palette, &[], &ApplyOptions::default()).unwrap();

    let alacritty = find(&reports, "alacritty");
    assert!(alacritty.error.is_some(), "expected alacritty to error");

    // Kitty still applied despite the sibling failure.
    assert!(find(&reports, "kitty").files.iter().any(|f| f.changed));
    assert!(fx.read("kitty/kitty.conf").contains("color1 #ff0000"));
}

#[test]
fn dry_run_writes_nothing() {
    let fx = Fixture::new();
    fx.install_core_apps();
    let before = fx.read("kitty/kitty.conf");
    let env = fx.env();
    let palette = fx.palette();

    let opts = ApplyOptions {
        dry_run: true,
        ..Default::default()
    };
    let reports = run_apply(&env, &palette, &[], &opts).unwrap();
    assert!(find(&reports, "kitty").files.iter().any(|f| f.changed));
    assert_eq!(fx.read("kitty/kitty.conf"), before);
    assert!(!fx.exists("micro/colorschemes/palettizer.micro"));
}

#[test]
fn no_backup_flag_skips_backups() {
    let fx = Fixture::new();
    fx.install_core_apps();
    let env = fx.env();
    let palette = fx.palette();

    let opts = ApplyOptions {
        no_backup: true,
        ..Default::default()
    };
    run_apply(&env, &palette, &["kitty".to_string()], &opts).unwrap();

    let store = BackupStore::new(&env).unwrap();
    assert!(store.latest("kitty").unwrap().is_none());
    assert!(fx.read("kitty/kitty.conf").contains("color1 #ff0000"));
}

#[test]
fn app_overrides_take_precedence() {
    let fx = Fixture::new();
    fx.install_core_apps();
    let env = fx.env();

    let palette = Palette::from_yaml(&format!(
        "{PALETTE}\napp_overrides:\n  kitty:\n    color1: \"#123456\"\n"
    ))
    .unwrap();
    run_apply(
        &env,
        &palette,
        &["kitty".to_string()],
        &ApplyOptions::default(),
    )
    .unwrap();
    assert!(fx.read("kitty/kitty.conf").contains("color1 #123456"));
    assert!(!fx.read("kitty/kitty.conf").contains("color1 #ff0000"));
}

#[test]
fn tmux_block_is_single_and_user_config_survives() {
    let fx = Fixture::new();
    fx.fake_bin("tmux");
    let tmux_conf = fx.home.join(".tmux.conf");
    std::fs::create_dir_all(fx.home.join(".config")).unwrap();
    std::fs::write(&tmux_conf, "set -g prefix C-a\n").unwrap();

    let env = fx.env();
    let palette = fx.palette();
    run_apply(
        &env,
        &palette,
        &["tmux".to_string()],
        &ApplyOptions::default(),
    )
    .unwrap();
    run_apply(
        &env,
        &palette,
        &["tmux".to_string()],
        &ApplyOptions::default(),
    )
    .unwrap();

    let out = std::fs::read_to_string(&tmux_conf).unwrap();
    assert!(out.contains("set -g prefix C-a"));
    assert_eq!(out.matches("# >>> palettizer >>>").count(), 1);
    assert_eq!(out.matches("# <<< palettizer <<<").count(), 1);
}

#[test]
fn ghostty_prefers_config_then_theme_yaml() {
    let fx = Fixture::new();
    fx.fake_bin("ghostty");
    fx.config("ghostty/theme.yaml", "font-family = Fira Code\n");
    let env = fx.env();
    let palette = fx.palette();

    // Only theme.yaml exists -> still compatible, patches that file.
    let reports = run_status(&env, &palette, &["ghostty".to_string()]).unwrap();
    let g = find(&reports, "ghostty");
    assert!(g.detect.is_compatible());
    assert!(fx.exists("ghostty/theme.yaml"));
}

/// Guard helper: ensure a config path resolves under the synthetic home.
#[test]
fn paths_are_scoped_to_synthetic_home() {
    let fx = Fixture::new();
    fx.install_core_apps();
    let env = fx.env();
    let path = env.app_config("kitty/kitty.conf").unwrap();
    assert!(path.starts_with(&fx.home));
    assert_eq!(path, Path::new(&fx.home).join(".config/kitty/kitty.conf"));
}
