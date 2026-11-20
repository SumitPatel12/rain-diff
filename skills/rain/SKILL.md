---
name: rain
description: Open a change in Rain (a native desktop code-review app) — one window over every repo of a story or review — pre-loaded with comment drafts, a verify checklist and an overview, stay attached while the user reviews so their questions get answered live, then read their verdicts and replies back. Use when the user says "open the diff in Rain", "review in Rain", "open it in Rain", "load the comments", "read my Rain comments", "close Rain", and whenever a review or implement flow reaches its hand-off to a human reviewer.
---

# Rain

Rain is a desktop diff reviewer. A flow hands it a diff plus comment drafts, the user reviews in the window (agree, dismiss, defer, reply, ask a question, Finish review), and the flow reads the result back. Full API, every CLI flag and HTTP route: the Rain repo's [`docs/agent.md`](../../docs/agent.md) — read the section you need instead of guessing a flag.

## Quickstart for agents

Never hand over a bare diff: in order, `open` → `brief set` (intent, how, a tour; docs §4) → `brief check` until it exits 0 (fix and set again) → `overview validate` until it exits 0 (then pass it with `-Overview` or `overview set`) → the watcher below. While the window has no brief, `rain open --background` prints `next: send a brief …` on stderr; a `brief-missing` event means the same.

## Configuration

- **`RAIN_EXE`** — path to the Rain binary. Default on Windows: `%LOCALAPPDATA%\Programs\Rain\Rain.exe` (where `install-rain.ps1` puts it). On macOS/Linux set `RAIN_EXE` (the installer's default there is `~/.local/bin/Rain`, which the scripts also try). Below, `$r` is that path.
- **`RAIN_REPO`** — a Rain checkout, used only by `install-rain.ps1`.
- **`RAIN_REPOS_ROOT`** — the folder whose child repos `rain-story.ps1 -Key` scans (or pass `-Root`).
- **Update Rain** after its main branch moves: `pwsh <skill dir>/install-rain.ps1 -Repo <rain checkout>` (builds, installs; refuses while an installed window is open). Don't run it unasked; if `installed-from.txt` beside the binary is behind the Rain checkout's `git log -1`, mention it once.
- Scripts need PowerShell 7+ (`pwsh`) and git.

## Sessions

**Session = `<flow>-<KEY>`** (`review-PROJ-123`, `implement-PROJ-123`). Every call takes `--session`; one session is one window, so two flows never read or close each other's. Reuse a session name only for the same repo set.

## Open, read back, close — `rain-story.ps1`

`<skill dir>` below is where this skill lives (for example `~/.claude/skills/rain`).

| Mode | Command | Does |
|---|---|---|
| open | `& <skill dir>/rain-story.ps1 -Mode open -Session <s> -Key <KEY> [-Root <folder>] [-Base origin/main] [-CommentsDir <dir>] [-Overview <abs path>]` | One window over every git repo directly under `-Root` (or `$env:RAIN_REPOS_ROOT`) whose checked-out branch contains the key. Diff = `HEAD` vs merge-base with `-Base`. Imports `<CommentsDir>/<repo-folder>.json` per repo. Prints `opened`/`reused`, the pid and the repo ids. Reopening reuses the window and adds only comments it doesn't already have. `read-only` (with a warning) means a second open raced the first: that extra window refuses every change with 423; keep using `--session` (it reaches the holder) and close the extra one with `& $r close --port <its port>`. |
| open (explicit) | `-Repos <path>,<path>` instead of `-Key` | Worktrees, review clones, repos on a branch without the key. |
| open (uncommitted) | add `-Uncommitted` | Target `.` with untracked files. |
| get | `-Mode get -Session <s> -OutDir <dir>` | Writes `<dir>/rain-comments.json`: every thread in **every** state, so you see what the user dropped. |
| close | `-Mode close -Session <s>` | Saves and closes that session's window only. Never close a session you didn't open. |

Tell the user the window is open once; there is no URL to hand over.

**Comment files** — `<CommentsDir>/<repo-folder>.json`, a JSON array:
`{"type":"thread","filePath":"<repo-relative>","position":{"side":"new","line":N | {"start":a,"end":b}},"body":"…"}`
- `side:"old"` only for lines that exist only on the deleted side.
- Start each body with the finding ID and label — `F3 issue (blocking): …`. Rain reads the label as the thread's kind and blocking flag, and the ID maps the readback to your finding.
- Optional: `"kind"` (`issue|suggestion|nit|question|praise|comment`), `"blocking"`; file-level or whole-change findings via `target` (docs §5.1).
- Never put secrets or credentials in a body.

**Verify checklist** (optional, recommended when the flow has manual test steps): push a Brief with only a Verify section; Rain shows it as a checklist floating over the diff, and the user ticks items off (ticks saved in the session). One list item per check, each a single action and its expected result:
```markdown
# <story title>
## Verify by hand
- Open the account page and add a voucher of 0: it is refused with "Amount must be positive".
- Add a voucher of 10: the account total rises by 10 after a reload.
```
Write it to a file and run `& $r brief set '<abs path>.md' --session <s>` after `open`. A Brief also fills the Overview tab, so add `## Intent` (one paragraph) when you have one; leave out sections you have nothing real for. Ticks are not part of comment readback. The user may reword, remove or add checks; a later `brief set` keeps their edits, and `& $r brief get --session <s>` → `verify.items[]` (`text`, `done`, `source`) is the list as they have it (docs §4.2).

**Overview** (optional): write it per [`references/overview-guide.md`](references/overview-guide.md), validate with `& $r overview validate <abs path>` (exit 1 prints the issues), pass `-Overview`. It drives the Map, per-file risk and intent notes. Include its `diagrams` block (guide §7a: tour almost always; flowchart, lineage, icicle when the change calls for them) — that is what puts the animated diagrams on the Map; without it the Map shows only the graph views. Repo ids come from the overview's `repos[].dir` matching the folder name. Never read back.

## Stay attached while the user reviews (live loop)

Right after opening, start the watcher **in the background** (`run_in_background`):
`pwsh -NoProfile -File <skill dir>/rain-watch.ps1 -Session <s>`
It holds `rain watch --for agent`, so the window shows the agent as attached, and exits only when something needs you:
- **exit 0** — JSON events. For each:
  - `ask` (`data.threadId`, `data.question`, `data.filePath`, `data.excerpt`): the user asked a question and expects an answer now.
    - **Claim it at once**, before reading any code: `& $r comment claim --session <s> --thread <threadId>`. A claimed question is yours: `claude -p` stays off it until you reply (up to `ask.timeoutSec`, 120 s), even while your watcher is down. For the one you answer yourself, also `& $r presence working --on <threadId> --session <s>`.
    - Main thread free: answer it. Read the code it points at, then `& $r comment reply --session <s> --thread <threadId> --body '…'` — ≤150 words, cite `path:line`, say when the answer isn't in the code you read. Don't edit files to answer.
    - Main thread busy with another question, or several asks in one batch: spawn one helper agent per extra question, in the background, with a self-contained brief: the question, `threadId`, file and excerpt, the session name, the review's intent, and a summary of anything already discussed. Each helper claims its thread, reads the code and replies with `comment reply --thread <id>`.
    - Restart the watcher straight away, so you never stop listening. `& $r presence idle --session <s>` once every answer is posted.
    - `claude -p` is only the fallback, for a question nobody claims within `ask.handoffSec` (20 s) or nobody answers within `ask.timeoutSec`. If you answer one it is running, it stops and your answer stands. If the thread already shows `answer.by: launcher` with a status other than `failed`, skip it.
    - No `threadId` and `data.request: "brief"`: the user pressed *Ask the agent to write one* on the Overview. Write the Brief (Intent, How, Verify as above; docs §4) and `& $r brief set '<abs path>' --session <s>`; no reply.
  - `brief-missing`: you have been attached about 2 minutes and no brief is set (sent once). Write the Brief now and `& $r brief set '<abs path>' --session <s>`, then `brief check` until clean; no reply.
  - `submitted` (`data.verdict`): the user pressed Finish review. Treat it as "done" — go to readback.
  - `"gap": true`: events fell off Rain's ring while you weren't watching; re-read the threads (`-Mode get`) before going on.
  Then restart the watcher with `-Since <seq>` from the output.
- **exit 3** — the window closed. Do the readback if you haven't (`get` fails once the window is gone; ask the user to reopen with the same `-Session`, which restores their threads).
- The user saying "done" in chat also ends the review. Stop the background watcher you started (its shell id) when the loop ends.

If you aren't attached, Rain answers questions itself after 20 s with a local `claude -p` (after the user allows it once), so a gap in the loop is never fatal.

## Readback — merge into the calling flow's artifact

Read `rain-comments.json` (`{"version","threads":[…]}`). Per thread: `messages[0]` is the original, later messages are replies; `repo` names the repo in a multi-repo window; `filePath` is repo-relative.

| The user did | Thread shows | Means |
|---|---|---|
| Agree (`a`) | `disposition: "agree"` | Keep as drafted. |
| Dismiss (`x`) | `disposition: "dismiss"` and/or `state: "dismissed"` (`dismissReason`) | Drop the finding; record the reason. |
| Ask (`?` on a draft) | `disposition: "ask"`, a question reply | Answer it (live loop); then keep the draft unless the user says otherwise. |
| Resolve | `state: "resolved"` | Drop the finding. |
| Defer (`d`) | `disposition: "defer"` | Not decided — ask the user. |
| Reply | extra `messages` by `author` other than `claude` | Instructions to apply (reword, fix, explain). |
| New thread | a thread whose `messages[0].author` isn't `claude` | A new item. |
| Nothing | no disposition, `state` absent (open) | Keep as drafted. |

Show the merged result as a short table and confirm before acting on it.

## After a fix (fix loops)

Commit, then for each thread the commit addresses: `& $r comment set-state <id> addressed --sha <short sha> --session <s>`, then `& $r reload --session <s>`. The window re-runs the diff in place, re-anchors threads, and the user confirms or reopens each "Addressed?" one. Never resolve the user's threads yourself.

## Rules
- The repo set comes from the story's branches (or the repos the review was given), never from "repos touched this session".
- Close the window when the calling flow ends, unless the user wants it kept.
- If `open` throws (Rain missing, git error), say so in one line and fall back to the calling flow's in-chat table. Never block the flow on Rain.
