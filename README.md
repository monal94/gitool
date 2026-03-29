# gitool

A lazygit-inspired TUI for managing multiple git repositories.

Built with Rust, [ratatui](https://github.com/ratatui/ratatui), and [libgit2](https://libgit2.org/).

```
┌────────────────────────────────────────────────────────────────┐
│ [1 Repos]  2 Files  3 Branches  4 Commits  5 Stash  workspace │
├───────────────────────┬────────────────────────────────────────┤
│ ┌ 1 Repos ──────────┐ │                                        │
│ │ ▸ repo-alpha  ● Δ2 │ │  Preview                               │
│ │   repo-beta   ●    │ │  (context-sensitive)                   │
│ │   repo-gamma  ↑1   │ │                                        │
│ └────────────────────┘ │  Repos → repo summary                  │
│ ┌ 2 Files ──────────┐ │  Files → syntax-highlighted diff       │
│ │ ● M src/main.rs    │ │  Branches → recent commits             │
│ │ ○ A README.md      │ │  Commits → commit diff                │
│ └────────────────────┘ │  Stash → stash diff                   │
│ ┌ 3 Branches ───────┐ │                                        │
│ │ ● main origin/main │ │                                        │
│ │   feat/login [↑2]  │ │                                        │
│ └────────────────────┘ │                                        │
│ ┌ 4 Commits ────────┐ │                                        │
│ │ abc1234 Fix bug 2m │ │                                        │
│ └────────────────────┘ │                                        │
│ ┌ 5 Stash ──────────┐ │                                        │
│ │ stash@{0}: WIP     │ │                                        │
│ └────────────────────┘ │                                        │
├───────────────────────┴────────────────────────────────────────┤
│ j/k nav  Tab panel  1-5 jump  a stage  c commit  q quit       │
└────────────────────────────────────────────────────────────────┘
```

## Features

| Category | Features |
|----------|----------|
| **Workspace** | Multi-repo view, workspace switching (`w`), repo hiding, search/filter (`/`), bulk mark (`Space`/`Ctrl+a`) |
| **Files** | Stage (`a`), unstage (`u`), discard (`x`), per-file diff (`d`/`Enter`), open in editor (`e`), blame (`b`) |
| **Branches** | Checkout (`Enter`), create (`n`), delete (`D`), rename (`R`), merge (`m`), auto-detect default branch |
| **Commits** | Browse history, cherry-pick (`C`), revert (`X`), create tag (`t`), copy hash (`y`), commit diff preview |
| **Stash** | Stash with message (`s`), pop (`Enter`), drop (`x`), browse stash diffs |
| **Git ops** | Pull (`p`), push (`P`), fetch (`f`), commit (`c`), amend (`A`) — all non-blocking via rayon |
| **UI** | 5 stacked panels + context preview, mouse support, syntax-highlighted diffs, <50ms startup |
| **Undo** | `Ctrl+z` reverses checkout and stash operations |

## Install

```bash
git clone https://github.com/monal94/gitool.git
cd gitool
cargo install --path .
```

## Usage

```bash
gitool ~/Projects/my-workspace   # open a workspace
gitool                           # current directory (single repo or workspace)
```

## Key Bindings

### Global (any panel)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `1`-`5` | Jump to panel | `Tab`/`BackTab` | Cycle panels |
| `q`/`Esc` | Quit | `r` | Refresh |
| `/` | Filter | `` ` `` | Command log |
| `w` | Switch workspace | `Ctrl+z` | Undo |

### Repos (1)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `j`/`k` | Navigate | `Enter` | Jump to Files |
| `p`/`P` | Pull / Push | `f` | Fetch |
| `Space` | Mark repo | `Ctrl+a`/`Ctrl+d` | Mark / Unmark all |

### Files (2)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `a` | Stage | `u` | Unstage |
| `x` | Discard | `d`/`Enter` | View diff |
| `c` | Commit | `A` | Amend commit |
| `e` | Open in editor | `b` | Blame view |

### Branches (3)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `Enter` | Checkout | `n` | New branch |
| `D` | Delete | `R` | Rename |
| `m` | Merge | `s` | Stash |

### Commits (4)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `d`/`u` | Scroll preview | `y` | Copy hash |
| `C` | Cherry-pick | `X` | Revert |
| `t` | Create tag | | |

### Stash (5)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `s`/`Enter` | Pop stash | `x` | Drop stash |

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
├── main.rs              # Event loop, key/mouse handlers
├── app/
│   ├── mod.rs           # App struct, enums, core state
│   ├── navigation.rs    # Panel switching, selection
│   ├── dispatch.rs      # Async background operations
│   ├── git_ops.rs       # Git action methods
│   ├── branch_ops.rs    # Branch CRUD, text input
│   ├── state.rs         # Refresh, workspace, load data
│   ├── watcher.rs       # File system watching
│   └── undo.rs          # Undo stack
├── git/
│   ├── mod.rs           # Re-exports, test helpers
│   ├── scan.rs          # Repo scanning, branch loading
│   ├── status.rs        # File status via libgit2
│   ├── diff.rs          # Diff generation via libgit2
│   ├── ops.rs           # Git mutations (libgit2 + CLI)
│   └── log.rs           # Commit log, blame, show files
├── config.rs            # YAML workspace config
├── types.rs             # Data models
├── highlight.rs         # Syntect diff highlighting
└── ui/
    ├── mod.rs           # Layout orchestrator
    ├── repo_list.rs     # Repos panel
    ├── files.rs         # Files panel
    ├── branches.rs      # Branches panel
    ├── commits.rs       # Commits panel
    ├── stash_panel.rs   # Stash panel
    ├── preview.rs       # Context preview (right)
    ├── diff.rs          # Full-screen diff overlay
    ├── blame.rs         # Blame overlay
    ├── command_log.rs   # Command history overlay
    ├── confirm.rs       # Confirmation dialog
    └── modal.rs         # Workspace switcher

```

Git reads use libgit2 directly (no subprocess). Mutations use rayon thread pool for non-blocking execution. Diffs are syntax-highlighted with windowed rendering (only visible lines processed). File watching auto-refreshes on `.git` changes with generation-counter caching.

## Testing

```bash
cargo test    # 166 tests
```

## Tech Stack

[ratatui](https://github.com/ratatui/ratatui) | [git2](https://github.com/rust-lang/git2-rs) | [syntect](https://github.com/trishume/syntect) | [rayon](https://github.com/rayon-rs/rayon) | [arboard](https://github.com/1Password/arboard) | [notify](https://github.com/notify-rs/notify) | [crossterm](https://github.com/crossterm-rs/crossterm) | [clap](https://github.com/clap-rs/clap)

## License

MIT
