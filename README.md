# Rain

Rain is a native desktop code-review app for git diffs, written in Rust on [Rune](https://github.com/anomalyco/rune).
One `Rain.exe` runs `git diff` itself, draws million-line diffs smoothly, and keeps comment threads, viewed state and
review rounds in a session file. It is built for reviewing alongside a coding agent:

- **Brief and Overview**: the agent writes a Brief (facts, intent, how it works with cited lines, invariants, what is
  not covered, how to verify) and an overview document; Rain checks every cited path against the diff.
- **Comments**: line, range, span, file and PR-level threads with kinds (issue, suggestion, nit, question, praise),
  states (open, addressed, resolved, dismissed) and replies; a Question is answered inline by the agent.
- **Map**: native architecture, data-flow and data-model diagrams from the overview, shaded by what you have viewed.
- **Tour**: a guided walk through the change, a spotlight sliding between the cited ranges.
- **Triage**: deterministic grouping (changes, tests, moves, formatting-only, generated, lockfiles, huge, binary) in
  one reading order.

An agent drives a running window over HTTP or the CLI; see [docs/agent.md](docs/agent.md). Rain also serves
difit's comment routes, so tooling written for difit can read comments back unchanged
([docs/compat.md](docs/compat.md)).

## Build and run

Rust 1.95.0 (pinned in `rust-toolchain.toml`), Windows + MSVC. Rune is a git dependency pinned to one revision.

```powershell
cargo build --release -p rain                          # target\release\Rain.exe
cargo run -p rain -- <target> <base> --merge-base      # review <target> against its merge base with <base>
cargo run -p rain -- HEAD                              # the last commit
cargo run -p rain -- .                                 # all uncommitted changes (also: working, staged)
cargo run -p rain -- HEAD main --repo <dir> --repo <dir2>   # one window over several repositories
cargo run -p rain -- --fixture fixtures/m.patch        # a .patch file instead of git
```

`<target>` is a commit-ish (or `.`, `working`, `staged`); `<base>` defaults to the target's parent. Useful flags:
`--session <id>` (threads and viewed state persist per session; a window already serving it is reused),
`--background` (print `{"port","url","pid"}` and exit), `--include-untracked`, `--ignore-whitespace`,
`--context <n>`, `--goto <repo>/<path>:<line>`, `--clean`. `rain open --last` reopens the most recent session.
`Rain.exe --help` lists every subcommand (`comment`, `brief`, `triage`, `note`, `overview`, `watch`, `review`, …).

Headless snapshots use Rune's own flags, which pass through argv:

```powershell
cargo run -p rain -- --snap out.png --scene light --scale 1.5   # headless snapshot
cargo run -p rain -- --tree                                     # element tree dump
```

Scenes: `files split zoomed composer composer-question thread-states orphan palette ruler filter wholefile notes
finish since-round reload-banner conversation presence brief brief-folded map board board-single picker error`
(`wholefile` needs `--repo`). Scenes start from default settings, so they follow `--theme dark|light`;
`--theme everforest|gruvbox-hard|gruvbox-medium` draws a scene in that theme. `RUNE_GPU_LOG=1` prints the adapter
and backend.

## Settings

`%LOCALAPPDATA%\Rain\settings.json`, written by the window (theme, layout and the rest are also in the palette,
`Ctrl+K`). Sessions live in `%LOCALAPPDATA%\Rain\sessions\`.

```json
{
  "theme": "system",
  "layout": "split",
  "density": "compact",
  "codeSize": "M",
  "editor": "code --goto \"{path}:{line}\"",
  "ask": { "enabled": true, "command": "claude -p --output-format text --allowedTools Read,Grep,Glob --add-dir {repos}" }
}
```

| Key | Values |
|---|---|
| `theme` | `system` (default; follows the OS), `light`, `dark`, `everforest`, `gruvbox-hard`, `gruvbox-medium`. An unknown value falls back to `system`. Pick one live under `⋯` › Appearance › Theme or with `Theme:` in the palette; Toggle theme flips between light and dark |
| `layout` | `split` (default; falls back to unified when a side is too narrow, and says so), `auto` (the same, silently), `unified` |
| `density` | `compact` (default), `comfortable` |
| `codeSize` | `S`, `M` (default), `L` |
| `editor` | open-in-editor command with `{path}` and `{line}` |
| `ask` | the local command that answers questions when no agent is attached (prompt on stdin, answer on stdout); `redact` globs are never sent |

## Using the window

The window is the list on the left and the reader on the right.

| Part | What |
|---|---|
| Toolbar | `repo · branch → base`, Must / Review progress, `N waiting` (`w`), the agent's presence dot, `Auto │ Split │ Unified`, `Map`, `Finish review…` |
| List | sections, each folding on its header: Read me first (Brief, Map) · Changes (triage order) · Tests · Moves · Formatting-only · Generated · Lockfile · Huge · Binary · Ignored · Done · **Waiting on you**. Group headers carry `[Accept N]`. The filter box and chips narrow every section and the reader |
| Reader | the Brief card on top (Facts, Intent, How with cite chips, Invariants, Not changed, Not covered, Verify, `▶ Tour`), then the files; `Shift+S` switches to one item at a time |
| Overlays | Conversation (`C`), Map (`m`), Tour (`T`), picker (`Ctrl+P`), Finish review (`Shift+F`) |
| Composer | `c` Comment / `?` Question tabs (`Ctrl+1/2`); the same composer opens on Brief sections, Map nodes and notes |

| Key | Does |
|---|---|
| `j`/`k`, `n`/`p`, `]`/`[` | line, hunk, file |
| `J`/`K`, `Alt+←`/`Alt+→` | next/previous list section; jump history |
| `Space` / `v` / `Shift+V` / `u` | viewed + next / toggle viewed / accept the file's group / undo the last accept |
| `w` | next thread waiting on you |
| `c`, `?`, `r`, `a x d` | comment, question, reply, Agree / Dismiss (reason) / Defer |
| `C`, `m`, `b`, `T` | Conversation, Map, fold the Brief, Tour |
| `}`/`{`, `H` | next/previous Map view; fold or open Verify by hand |
| `s` / `Shift+S` / `l` / `Shift+Esc` | split ↔ unified / stream ↔ one at a time / list ↔ rail / zoom and restore |
| `f` or `Ctrl+L` / `Ctrl+Shift+L` | filter files / clear the filter |
| `Ctrl+P` / `Ctrl+K` | picker / palette |
| `e` / `E` | show the current file whole / open every file's context |
| `/`, `y`, `Ctrl+O` | find, copy `<repoId>/<path>:<line>`, open in the editor |
| `Shift+R`, `Shift+F`, `F1`, `Ctrl+D`, `Esc` | reload in place, Finish review, keyboard sheet, density, close the top layer |

## Layout

| Path | What |
|---|---|
| `crates/rain-core` | data model: diff arena, comments/threads, session, overview, tags, launch, keymap |
| `crates/rain-git` | git CLI driver + streaming unified-diff parser |
| `crates/rain-diffview` | virtual diff view (vendored from rune-kit, MIT; see `VENDORED.md`) |
| `crates/rain-syntax` | syntect + two-face highlighting |
| `crates/rain-comments` | session store, import/export formats, thread UI |
| `crates/rain-tree` | file tree, viewed checkboxes, tag filter |
| `crates/rain-diagram` | the Map: architecture / data-flow / data-model diagrams |
| `crates/rain-triage` | deterministic triage: groups with reasons, reading order, facts |
| `crates/rain-overview` | overview validation, coverage, tags, chapters, Brief check and tour steps |
| `crates/rain-ask` | runs the local command that answers questions |
| `crates/rain-server` | difit-compatible HTTP routes plus the agent routes |
| `crates/rain-cli` | CLI: open/reuse/`--last`, background handshake, every agent subcommand |
| `crates/rain-app` | the Rune window |
| `crates/rain` | the `Rain.exe` binary (its `build.rs` embeds the icon and version info) |
| `assets/icon` | `rain.svg`, `rain-small.svg` and the generated `rain.ico` |
| `xtask` | fixture and icon generator |

## Development

```powershell
pwsh scripts/gates.ps1          # fmt --check, clippy -D warnings, test, build --release
pwsh scripts/gates.ps1 -Perf    # plus the #[ignore] release perf tests
cargo xtask fixtures --tier s   # regenerate fixtures/s.patch (also: m, l, xl)
cargo xtask icon                # rebuild assets/icon/rain.ico from the SVGs
```

Fixture tiers write a seeded scratch repo to `fixtures/gen/<tier>/` (ignored); `s` and `m` are also copied to
`fixtures/s.patch` and `fixtures/m.patch`. The sample code in the fixtures is synthetic.

## License

Copyright (C) 2026 Sumit Patel.

Rain is licensed under the GNU Affero General Public License, version 3 only (`AGPL-3.0-only`); see
[LICENSE](LICENSE).

`crates/rain-diffview` is vendored and adapted from rune-kit (part of [Rune](https://github.com/anomalyco/rune))
and stays under the MIT license, Copyright (c) 2026 Anomaly; see
[crates/rain-diffview/LICENSE-rune](crates/rain-diffview/LICENSE-rune) and
[crates/rain-diffview/VENDORED.md](crates/rain-diffview/VENDORED.md).
