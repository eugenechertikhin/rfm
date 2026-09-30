# Rust File Manager

A lightweight terminal file manager for unix-like systems. Written in Rust
(ratatui + crossterm), fast, low on resources, no heavy dependencies.

## Features

- **Panels** — from 1 to N side-by-side (or stacked) panels, created and closed on demand; flat or tree view, 1–6 columns, sorting by name/size/date.
- **Command line** at the bottom — just start typing; `Enter` runs the command on the real shell screen (alternate screen buffer keeps your shell history intact). Persistent command history with frequency-based search.
- **VFS** — a panel can show a local directory, an archive (zip, tar, … — listed via configurable commands), or a remote host: `cd sftp://user@host/path` (OpenSSH ControlMaster), `cd ftp://…` (curl), `cd http(s)://…` (autoindex).
- **Built-in viewer** — lazy file reading (large files OK), hex dump, line wrap, text search, JSON/XML prettify.
- **Built-in editor** — simple in-memory editor with mouse support; explicit save, unsaved changes are marked and discarded on plain close.
- **Mouse** — click to focus/position cursor, double-click to open, wheel to scroll everywhere.
- **Themes** — configurable color themes (`default` dark and `mc` blue built in), state and layout persist across restarts.

## Build

```sh
cargo build --release
./target/release/rfm
```

## Packaging

```sh
make debug      # cargo build
make release    # cargo build --release
make debian     # build/rfm_<version>_<arch>.deb        (needs dpkg-deb)
make arch       # build/arch/rfm-<version>-1-*.pkg.tar.* (needs makepkg, run as non-root)
make rpm        # build/rpm/RPMS/<arch>/rfm-<version>-1.*.rpm (needs rpmbuild)
make clean
```

The version comes from the `VERSION` file. `arch` and `rpm` build from a
`git archive` of `HEAD`, so commit your changes first.

## Keys (essentials)

No DOS-style function-key shortcuts (F1, …) — only Ctrl shortcuts, to reuse muscle memory from the shell and/or Emacs.

| Key | Action |
|-----|--------|
| `Tab` | switch panel |
| `Enter` | open dir / run executable / enter archive / run typed command |
| `Ctrl+x` … | operations: `c` copy, `m` move, `d` delete, `n` mkdir, `v` view, `e` edit, `s` sort, `p` panels, `x` settings, `h` help, `q` quit |
| `Ctrl+t`, `Shift+↑/↓` | mark files |
| `Ctrl+s` | incremental search in list |
| `Ctrl+p`/`Ctrl+n`, `Ctrl+g` | command history prev/next, history search |
| `Shift+Tab` | filename completion |
| `Ctrl+o` | show shell screen |
| `Ctrl+q` | quit |

Full list: `Ctrl+x h` inside the app.

## Configuration

- Config: `$RFM_CONFIG` or `~/.config/rfm/config.toml` (created with defaults on first run) — themes, archive types, viewer/editor programs, layout.
- State (panels, history): `~/.local/share/rfm/`.

## License

GNU GPL 3.0

