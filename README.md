# Palettizer

Palettizer keeps a **single source-of-truth color palette** in sync across the
TUI apps you use. You edit one YAML file; `palettizer apply` patches the color
keys of every compatible app, backing up each file first.

- **Per-user**, config-file based — no daemons, no system-wide installs.
- **Apply on demand** (`palettizer apply`) — nothing watches your filesystem.
- **Non-destructive** — only known color keys are touched; everything else in
  your config is preserved byte-for-byte, and every changed file is backed up.
- **CLI-only** for now; a TUI selector is planned but out of MVP scope.

## Supported apps

| App | Config file | Strategy |
|-----|-------------|----------|
| kitty | `~/.config/kitty/kitty.conf` | in-place `colorN`/`foreground`/`background`/`cursor` |
| foot | `~/.config/foot/foot.ini` | in-place `[colors]` (`regularN`/`brightN`, legacy `colorN`) |
| alacritty | `~/.config/alacritty/alacritty.toml` | TOML round-trip via `toml_edit` (comments preserved) |
| micro | `~/.config/micro/settings.json` + generated colorscheme | sets `colorscheme` + writes `colorschemes/palettizer.micro` |
| ghostty | `~/.config/ghostty/config` or `theme.yaml` | in-place directives + `palette = N=#hex` |
| superfile | `~/.config/superfile/config.yaml` | nested `color:` mapping |
| btop | `~/.config/btop/btop.conf` + generated theme | template-generated `themes/palettizer.theme` |
| tmux | `~/.tmux.conf` or `~/.config/tmux/tmux.conf` | managed, sentinel-delimited block |

## Install

```sh
cargo build --release
install -Dm755 target/release/palettizer ~/.local/bin/palettizer
```

## The palette file

Default location: `~/.config/palettizer/palette.yaml`
(override with `--config <path>` or `$PALETTIZER_CONFIG`). A sample lives in
[`examples/palette.yaml`](examples/palette.yaml).

```yaml
name: "dracula"
origin: https://github.com/dracula

palette:
  black:        "#282a36"
  red:          "#ff5555"
  green:        "#50fa7b"
  yellow:       "#f1fa8c"
  blue:         "#bd93f9"
  magenta:      "#ff79c6"
  cyan:         "#8be9fd"
  white:        "#f8f8f2"
  brightBlack:  "#44475a"
  brightRed:    "#ff6e67"
  brightGreen:  "#69ff94"
  brightYellow: "#ffff86"
  brightBlue:   "#d6acff"
  brightMagenta: "#ff92df"
  brightCyan:   "#a4ffff"
  brightWhite:  "#ffffff"
  foreground:   "#f8f8f2"
  background:   "#282a36"
```

Canonical keys are the 16 ANSI colors plus `foreground`/`background`. Key
matching is tolerant (`brightBlack`, `bright_black`, `brightblack` all work),
and colors accept `#rgb` or `#rrggbb`. Unknown keys are rejected so typos fail
loudly instead of doing nothing.

### Per-app overrides

Some apps need keys that are not part of the canonical palette. Use
`app_overrides`; overrides are applied last and always win.

```yaml
app_overrides:
  kitty:
    cursor: "#f8f8f2"
  tmux:
    # becomes: set -g @palettizer_base00 "#282a36"
    base00: "#282a36"
  alacritty:
    # dotted TOML path
    colors.selection.background: "#44475a"
  ghostty:
    palette5: "#ff79c6"   # override a single palette slot
```

## Usage

```sh
palettizer list              # what's detected and what it would receive
palettizer status            # per-app: up to date / needs apply + keys touched
palettizer apply             # apply to every compatible app
palettizer apply kitty --diff
palettizer apply --dry-run
palettizer rollback tmux     # restore the most recent backup
```

Compatibility is detected with **binary on `$PATH` AND config file/dir exists**.
Apps that aren't compatible are reported with a reason and skipped; a failure in
one adapter never aborts the rest of the run.

## Backups & rollback

Before overwriting anything, each changed file is copied to:

```
~/.config/.palettizer/backups/<app>/<file>.<timestamp>.bak
~/.config/.palettizer/backups/<app>/<timestamp>.json   # manifest
~/.config/.palettizer/backups/<app>/latest.json        # newest manifest
```

`palettizer rollback <app>` restores everything from `latest.json`. Files the
adapter *created* (for example a generated btop or micro theme) are deleted on
rollback rather than restored.

## Design notes

- **Portable paths.** All path resolution goes through `Env`
  (`$XDG_CONFIG_HOME` → `$HOME/.config`), so adding macOS/Windows roots later is
  additive. Core logic never uses Linux-specific syscalls.
- **Patch, don't regenerate.** Adapters edit only known keys. `status` lists the
  keys that were touched so you can audit non-obvious mappings (tmux, btop).
- **Template mode where patching is fragile.** tmux is managed as a sentinel
  block; btop themes are generated whole.

### Known limitations

- **micro:** the plan described storing colors in `settings.json`. Micro actually
  loads colors from `colorschemes/<name>.micro`, so this adapter generates a real
  colorscheme and points `settings.json` at it — the only way micro will actually
  display the palette.
- **superfile:** only `config.yaml` is patched; a `config.toml`-only install is
  reported as *unsupported* rather than guessed at.
- **alacritty:** only `alacritty.toml` is patched; legacy `alacritty.yml` is
  reported as *unsupported*.

## Development

```sh
cargo test          # unit + end-to-end tests (isolated synthetic $HOME)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build --release
```

Tests never touch your real config: they build a synthetic `$HOME`, `$PATH`, and
`$XDG_CONFIG_HOME` via `Env`.
