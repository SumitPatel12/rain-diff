# Rain skill for Claude Code

A [Claude Code](https://docs.claude.com/en/docs/claude-code) skill that drives Rain from an agent: it opens
one Rain window over every repo of a story or review, loads comment drafts, a verify checklist and an
overview, stays attached while you review so your questions get answered live, then reads your verdicts
and replies back into the calling flow.

This folder is a template. Copy it, then adapt the parts listed under [Adapt it](#adapt-it).

| File | What |
|---|---|
| `SKILL.md` | The skill: open / live loop / readback / fix loop / rules |
| `rain-story.ps1` | Open a session over one or more repos, read every thread back, close it |
| `rain-watch.ps1` | Wait as the attached agent until the user asks a question or finishes the review |
| `install-rain.ps1` | Build Rain from a checkout and install the binary where the scripts look for it |
| `references/overview-guide.md` | How to write the overview document behind Rain's Map |

The full CLI and HTTP API is in [`docs/agent.md`](../../docs/agent.md).

## Requirements

- PowerShell 7+ (`pwsh`) on `PATH`
- git
- Rain, either built from this repo (needs the Rust toolchain in `rust-toolchain.toml`) or an existing binary

## Install

1. Copy the skill into your Claude Code skills folder:
   ```powershell
   Copy-Item -Recurse <rain checkout>/skills/rain ~/.claude/skills/rain
   ```
   The `docs/agent.md` and `fixtures/` links in the copied files point back into the Rain repo; they
   resolve when you read the files here.
2. Install Rain where the scripts look for it:
   ```powershell
   pwsh ~/.claude/skills/rain/install-rain.ps1 -Repo <rain checkout>
   ```
   It builds `cargo build --release -p rain` into `<repo>/target` (or `-TargetDir`) and copies the binary to
   `%LOCALAPPDATA%\Programs\Rain\Rain.exe` on Windows or `~/.local/bin/Rain` elsewhere (or `-InstallDir`),
   with an `installed-from.txt` beside it. It refuses while an installed Rain window is open. `-From <file>`
   installs an existing build instead.
   Or skip it and set `RAIN_EXE` to a Rain binary you already have.

## Configuration

| Variable | Used by | Meaning |
|---|---|---|
| `RAIN_EXE` | all scripts | Path to the Rain binary. Default: `%LOCALAPPDATA%\Programs\Rain\Rain.exe` on Windows, `~/.local/bin/Rain` elsewhere, then `Rain` on `PATH`. Set it on macOS/Linux or for a custom location. |
| `RAIN_REPO` | `install-rain.ps1` | The Rain checkout to build (or pass `-Repo`). |
| `RAIN_REPOS_ROOT` | `rain-story.ps1` | The folder whose child git repos `-Key` scans (or pass `-Root`; default the current folder). |

## Adapt it

- **Session naming.** Sessions are `<flow>-<KEY>` (`review-PROJ-123`, `implement-PROJ-123`) so two flows never
  share a window. Use whatever key your tracker gives you.
- **Base branch.** `rain-story.ps1 -Base` defaults to `origin/main`. Change the default if your trunk is
  `origin/master`, `origin/develop`, …
- **Repo discovery.** `-Key` opens every git repo directly under `-Root` whose checked-out branch contains the
  key. If your repos live elsewhere or your branches don't carry the key, pass `-Repos <path>,<path>`, or
  change the discovery block in `rain-story.ps1`.
- **Overview guide.** `references/overview-guide.md` uses a made-up `api` / `web` / `mobile` product. Replace
  the lanes, the generated-file ignore table and the critical-domain list (§5, §5a) with your own so risk
  ratings match your codebase.
- **Hand-offs.** `SKILL.md`'s description lists the phrases that trigger it. If you have review or implement
  skills of your own, have them call this skill at their review hand-off.
