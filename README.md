# gitool

A lazygit-inspired TUI for managing multiple git repositories.

Built with Rust, [ratatui](https://github.com/ratatui/ratatui), and [libgit2](https://libgit2.org/).

```
┌─ WORKSPACE: sentry ─ ~/Projects/sentry ──────────────────────────┐
│                                                                    │
│  Repos              │  backend (main) ↑0 ↓0 Δ0 Stash:0           │
│  ─────              │  ───────────────────────────────────         │
│  > ai          ●    │  Branches                                    │
│    backend     ●    │  ● main origin/main                          │
│    bundle      Δ1   │    feat/auth origin/feat/auth [↑2]          │
│    core        ●    │    fix/bug [local]                           │
│    frontend    ●    │    feat/gis origin/feat/gis [remote only]   │
│    platform    ●    │                                              │
│                     │                                              │
├─────────────────────┴──────────────────────────────────────────────┤
│ j/k:nav  Tab:panel  Enter:checkout  p:pull  P:push  f:fetch       │
│ s:stash  d:diff  l:log  c:commit  z:zoom  `:cmdlog  q:quit        │
└────────────────────────────────────────────────────────────────────┘
```

## Features

| Category | Features |
|----------|----------|
| **Workspace** | Multi-repo view with status glyphs, workspace switching (`w`), repo hiding (`h`/`H`), search/filter (`/`) |
| **Branches** | Unified local+remote display, drift tracking vs `main` and remote, checkout (`Enter`), create (`n`), delete (`D`), rename (`R`), merge (`m`) |
| **Files** | Stage (`a`), unstage (`u`), discard (`x`) individual files with per-file status |
| **Git ops** | Pull (`p`), push (`P`), fetch (`f`), stash (`s`), diff (`d`), commit (`c`) — all non-blocking |
| **Bulk** | Multi-select repos (`Space`/`Ctrl+a`/`Ctrl+d`), bulk pull/push/fetch |
| **History** | Commit log (`l`), command log (`` ` ``), undo (`Ctrl+z`) |
| **UI** | Mouse support, zoom mode (`z`), file watching auto-refresh, <50ms startup |

## Install

```bash
git clone https://github.com/monal94/gitool.git
cd gitool
cargo install --path .
```

## Usage

```bash
gitool ~/Projects/my-workspace   # open a workspace
gitool                           # current directory
```

## Key Bindings

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `j`/`k` | Navigate | `Tab` | Switch panel |
| `Enter` | Checkout branch | `p`/`P` | Pull / Push |
| `f` | Fetch | `s` | Stash / Pop |
| `d` | Diff | `l` | Commit log |
| `c` | Create commit | `n` | New branch |
| `D` | Delete branch | `R` | Rename branch |
| `m` | Merge branch | `a`/`u`/`x` | Stage / Unstage / Discard |
| `Space` | Mark repo | `Ctrl+a`/`Ctrl+d` | Mark / Unmark all |
| `Ctrl+z` | Undo | `z` | Zoom panel |
| `/` | Filter | `` ` `` | Command log |
| `w` | Switch workspace | `h`/`H` | Hide / Show hidden |
| `r` | Refresh | `q`/`Esc` | Quit |

**Overlays** (`d`/`l`/`` ` ``): `j`/`k` scroll, `d`/`u` page, `Esc` close.

## Configuration

Auto-created at `~/.config/ws/config.yaml`:

```yaml
workspaces:
  sentry:
    path: ~/Projects/sentry
    hidden_repos: [docs]
defaults:
  workspace: sentry
```

## Architecture

```
src/
├── main.rs            # Event loop, key/mouse handlers
├── app.rs             # State, navigation, async dispatch, undo
├── git.rs             # Git operations (libgit2 reads, git CLI writes)
├── config.rs          # YAML config
├── types.rs           # RepoStatus, BranchEntry, FileEntry
└── ui/
    ├── mod.rs         # Layout, header, footer, zoom
    ├── repo_list.rs   # Repo list with status glyphs
    ├── detail.rs      # Branch list with drift
    ├── files.rs       # File staging panel
    ├── diff.rs        # Diff overlay
    ├── commit_log.rs  # Commit history
    ├── command_log.rs # Command history
    ├── modal.rs       # Workspace switcher
    └── confirm.rs     # Confirmation dialog
```

All git mutations run in background threads via `mpsc` channels. Repos scan in parallel at startup. Branch drift computes lazily on selection. File watching via `notify` auto-refreshes on `.git` changes.

## Tech Stack

[ratatui](https://github.com/ratatui/ratatui) | [crossterm](https://github.com/crossterm-rs/crossterm) | [git2](https://github.com/rust-lang/git2-rs) | [notify](https://github.com/notify-rs/notify) | [clap](https://github.com/clap-rs/clap) | [serde_yaml](https://github.com/dtolnay/serde-yaml)

## License

MIT
