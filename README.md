# gitool

A lazygit-inspired TUI for managing multiple git repositories from a single terminal interface.

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
│ j/k:nav  Enter:checkout  p:pull  P:push  f:fetch  s:stash  d:diff │
│ /:filter  w:workspace  h:hide  H:show hidden  r:refresh  q:quit   │
└────────────────────────────────────────────────────────────────────┘
```

## Features

- **Multi-repo workspace view** — see all repos at a glance with branch, dirty, ahead/behind, stash status
- **Unified branch display** — local and remote refs shown together (like `git log --decorate`)
- **Branch drift tracking** — see how far each branch is from `main` and its remote
- **Git actions** — pull, push, fetch, stash, checkout, diff without leaving the TUI
- **Non-blocking operations** — all git mutations run in background threads; UI never freezes
- **Parallel repo scanning** — startup is fast even with 12+ repos
- **Workspace switching** — manage multiple workspace directories, switch with `w`
- **Repo hiding** — hide repos you don't care about, persisted to config
- **Search/filter** — press `/` to filter repos or branches by name
- **Confirmation dialogs** — destructive actions (push, stash pop) require confirmation
- **Diff viewer** — syntax-highlighted scrollable diff overlay
- **Instant startup** — native Rust binary, <50ms cold start

## Installation

### Build from source

Requires [Rust](https://rustup.rs/) 1.63+.

```bash
git clone https://github.com/monal94/gitool.git
cd gitool
cargo install --path .
```

The binary is installed to `~/.cargo/bin/gitool`.

### Shell aliases (optional)

Add to your `~/.zshrc` or `~/.bashrc`:

```bash
alias ss='gitool ~/Projects/sentry'
alias hs='gitool ~/Projects/helix-workspace'
```

## Usage

```bash
# Open a workspace directory
gitool ~/Projects/my-workspace

# Or from the current directory
cd ~/Projects/my-workspace && gitool
```

### Key Bindings

#### Normal Mode

| Key | Action |
|-----|--------|
| `j` / `k` / `↓` / `↑` | Navigate repos or branches |
| `Tab` | Switch panel (repos / branches) |
| `Enter` | Checkout selected branch |
| `p` | Pull |
| `P` | Push (with confirmation) |
| `f` | Fetch all remotes |
| `s` | Stash (if dirty) / Pop stash (if clean, with confirmation) |
| `d` | Show diff |
| `/` | Filter repos or branches |
| `w` | Switch workspace |
| `h` | Hide/unhide repo |
| `H` | Toggle showing hidden repos |
| `r` | Refresh all repos |
| `q` / `Esc` | Quit |

#### Diff View

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll line by line |
| `d` / `u` | Page down / up |
| `q` / `Esc` | Close diff |

#### Filter Mode

| Key | Action |
|-----|--------|
| Type | Filter by substring |
| `Enter` | Confirm filter (keep active) |
| `Esc` | Clear filter |

## Configuration

Config is stored at `~/.config/ws/config.yaml` and auto-created on first run.

```yaml
workspaces:
  sentry:
    path: ~/Projects/sentry
    hidden_repos: []
  helix:
    path: ~/Projects/helix-workspace
    hidden_repos:
      - docs
defaults:
  workspace: sentry
```

- **workspaces** — named workspace entries with their paths
- **hidden_repos** — repos to hide per workspace (toggle with `h`)
- **defaults.workspace** — default workspace when no path is provided

## Architecture

```
src/
├── main.rs          # Entry point, event loop, key handlers
├── app.rs           # App state, navigation, async git dispatch
├── git.rs           # Git operations (libgit2 + git CLI)
├── config.rs        # YAML config load/save
├── types.rs         # Data models (RepoStatus, BranchEntry)
└── ui/
    ├── mod.rs       # Layout: header + panels + footer
    ├── repo_list.rs # Left panel: repo list with status glyphs
    ├── detail.rs    # Right panel: branch list with drift
    ├── diff.rs      # Diff overlay with syntax highlighting
    ├── modal.rs     # Workspace switcher modal
    └── confirm.rs   # Confirmation dialog
```

**Git operations** use `libgit2` (via the `git2` crate) for read operations (scanning, branch enumeration, status) and shell out to `git` CLI for mutations (pull, push, fetch, checkout, stash) since `git2` doesn't handle remote auth well.

**Non-blocking I/O**: All git mutations are dispatched to background threads via `std::sync::mpsc` channels. The event loop polls for results each tick, keeping the UI responsive.

**Parallel scanning**: Repos are scanned concurrently using `std::thread::scope` on startup.

## Roadmap

### Implemented

| Feature | Status |
|---------|--------|
| Multi-repo workspace view | Done |
| Unified branch display (local + remote) | Done |
| Branch drift tracking (vs main, vs remote) | Done |
| Git pull / push / fetch / stash / checkout | Done |
| Diff viewer | Done |
| Workspace switching | Done |
| Repo hiding (persisted) | Done |
| Search/filter (`/`) | Done |
| Confirmation dialogs | Done |
| Non-blocking async git ops | Done |
| Parallel repo scanning | Done |

### Planned

| Feature | Priority | Description |
|---------|----------|-------------|
| Files panel | P1 | Stage/unstage/discard individual files |
| Bulk operations | P1 | Select multiple repos, run git ops on all at once |
| Command log | P1 | Show executed git commands (toggle with `` ` ``) |
| Lazy drift calculation | P1 | Compute branch drift on-demand for faster startup |
| Commit log | P2 | View commit history, create commits |
| Branch actions | P2 | Create, delete, rename, merge branches |
| File watching | P2 | Auto-refresh when repos change externally |
| Render optimization | P2 | Only redraw on state changes |
| Mouse support | P3 | Click to select, scroll with wheel |
| Undo | P3 | Press `z` to undo last git operation |
| Zoom mode | P3 | Full-screen detail view for a single repo |
| Git object caching | P3 | Reuse libgit2 handles across scans |

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust 1.63+ |
| TUI framework | [ratatui](https://github.com/ratatui/ratatui) |
| Terminal backend | [crossterm](https://github.com/crossterm-rs/crossterm) |
| Git library | [git2](https://github.com/rust-lang/git2-rs) (libgit2 bindings) |
| Config | [serde_yaml](https://github.com/dtolnay/serde-yaml) |
| CLI | [clap](https://github.com/clap-rs/clap) |

## License

MIT
