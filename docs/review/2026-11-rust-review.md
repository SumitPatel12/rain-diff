# Rust review: whole Rain workspace (2026-11)

Rain is a native diff and code-review **application** (edition 2024, rust-version 1.95, MSVC), so this review applies app-level rules: no `#[non_exhaustive]`, sealed traits or docs on every public item. The review covered all 13 crates, read in full and taken crate by crate. Each finding below was re-read at the cited line and checked against its callers before it was kept.

**Verdict:** the code is in good shape. The server's CSRF and DNS-rebinding checks hold. Git argument handling is careful. Session saves are atomic and serialised. No `unsafe` was missing a `SAFETY` comment. The most important bug: a window opened from a **subfolder** of a checkout silently dropped files from the diff once it had 16 or more changes. The open-in-editor path was not command-injectable. But cmd expanded `%VAR%` inside file paths, which could open the wrong file.

**Tools run on `bfaf664`:**
- `cargo fmt --check`: clean.
- `cargo clippy --workspace --all-targets -W clippy::pedantic`: 795 warnings, none at default level.
  - Most are cast-truncation (≈230), `must_use` candidates (≈220), long literals and single-char names.
  - The ones that could matter were checked by hand. None were real bugs:
    - myers `isize`/`usize`
    - `state.rs` `i64`→`u32`
    - `f32` midpoints
    - `clock.rs` and `civil_from_days` casts
    - `ruler.rs:211`, `palette.rs:63`
    - `generated.rs` case-sensitive extension checks, which mirror difit on purpose
- `cargo test --workspace`: all passing.

Severity counts:

| Severity | Found | Fixed | Skipped |
|---|---|---|---|
| Must fix | 3 | 3 | 0 |
| Should fix | 9 | 9 | 0 |
| Consider | 15 | 4 | 11 |

## Must fix

### 1. A window opened in a subfolder loses files (`crates/rain-git/src/repo.rs:183-195`, `crates/rain-app/src/load.rs:181`)
**Rule:** correctness. Git pathspecs are relative to the working directory (`git help glossary`, "pathspec").
**Problem:** `diff_patch` lists the changed paths with `--name-status --no-relative`, which gives top-relative paths. It then hands them back as pathspecs to parallel `git -C <dir> diff -- <paths>`, where git reads them relative to `<dir>`. Launched from `repo\src`, every group matches nothing.

Verified: `git -C crates diff --no-relative HEAD~3 HEAD -- crates/rain-cli/src/exit_log.rs` printed nothing, while the same command from the top printed the file.

So with 16 or more changed files, or with `-w`, those files silently vanish. The same top-vs-subfolder mix-up also affects:
- working-tree `blob` reads (`repo.join(path)`)
- `untracked` (`ls-files` lists only the subfolder)
- open-in-editor (`repo.dir.join(path)`)

**Fix (done):** add `rain_git::toplevel(dir)` (`rev-parse --show-toplevel`, normalised). `load_repo` runs git and stores `RepoInfo.dir` at the top. `name` and `repository_id` still come from the launch folder, so existing sessions keep their ids. Regression test: `a_subfolder_launch_diffs_the_whole_checkout`.

### 2. Stale find hits index the wrong diff and can panic the UI thread (`crates/rain-app/src/app/find.rs:34`, `:81-82`)
**Rule:** panics on reachable paths (M-PANIC-ON-BUG).
**Problem:** `run_find` skipped work when the query text was unchanged, but `hits` index the *active* diff. That diff changes when you enter a chapter, filter, or reload. `/foo` in Files mode, then Tab into a chapter, then `/` Enter indexed `files()[f]` of the old diff: an out-of-bounds panic, or a jump to the wrong file.
**Fix (done):** the cache is keyed on the active diff's file pointers as well as the text, and `find_step` uses `get`. Regression test: `find_hits_follow_the_diff_on_screen`, which failed before the fix with `stale hit (2, 30)`.

### 3. An unreadable session file was replaced by an empty one and then saved over (`crates/rain-app/src/app/mod.rs:199`)
**Rule:** errors that lose data (`unwrap_or_default` on a fallible load).
**Problem:** `SessionFile::load(path).unwrap_or_default()` dropped `InvalidData` and installed an empty session. Autosave then wrote the empty session over the user's threads at the first change. This applies to the standalone `rain-app` entry; the CLI already refuses to start on a corrupt file.
**Fix (done):** the unreadable file is renamed to `<id>.json.corrupt` and reported, then an empty session starts. Regression test: `an_unreadable_session_file_is_kept_aside_not_overwritten`.

## Should fix

### 4. Thread line maps treated paths as globs (`crates/rain-git/src/repo.rs:546-553`)
**Rule:** argument handling; git `--literal-pathspecs`.
**Problem:** `line_map` ran `git diff -U0 … -- <path>` without `--literal-pathspecs`. For a Next.js route such as `r/[id].ts`, the pathspec also matched `r/i.ts` and `r/d.ts`, and their hunks moved the thread to the wrong lines. Every other diff call already passed the flag.
**Fix (done):** prepend `--literal-pathspecs`. Regression test: `line_maps_take_paths_literally`, which failed before the fix.

### 5. Open-in-editor: cmd expanded `%VAR%` in file paths (`crates/rain-app/src/app/editor.rs:134-158`)
**Rule:** command and argument injection (review focus).
**Problem:** every editor ran through `cmd /D /S /C "<quoted argv>"`.

What was already safe:
- `cmd_quote` quotes `& | < > ( ) ^` and spaces correctly.
- `"` cannot appear in a Windows file name, so a path can't break out of the quotes.
- Paths are absolute, so none starts with `-`.

What was broken:
- cmd expands `%NAME%` even inside quotes, and has no escape for it on a `/C` line. For a user named `reviewer`, a repo file `a%USERNAME%.cs` opened `areviewer.cs`.
- `!` is expanded the same way when delayed expansion is on in the registry.

This was not code execution: `:` is illegal in file names, so substring and replace expansions can't be formed.
**Fix (done):**
- Resolve `argv[0]` on `PATH` with `PATHEXT` and start it directly. For an `.exe`, std quotes the arguments. For a `.cmd` or `.bat`, std ≥ 1.77.2 escapes `%` itself and rejects arguments it can't make safe.
- Only a program that isn't found (a cmd built-in such as `start`) still goes through cmd. That path now passes `/V:OFF` and refuses arguments containing `%` or a newline.

Regression test: `a_batch_editor_gets_paths_with_cmd_metacharacters_verbatim` runs a real `.cmd` with `C:\r\a&b %USERNAME% (x) ^y!.cs` and checks it receives the path byte for byte.

### 6. A `settings.json` with any error was reset to defaults and then saved over (`crates/rain-app/src/settings.rs:80-92`, `app/mod.rs:190`)
**Rule:** errors that lose information.
**Problem:** a typo (for example a trailing comma) made serde reject the whole file. The next pane drag or theme toggle then saved the defaults over it, which deleted the hand-written `editor` line.
**Fix (done):** add `Settings::try_load`, which treats a missing file as the defaults and returns an error otherwise. On an error, `App::new` uses the defaults for this run, stops saving settings, and shows `settings ignored: <path>: <serde error>`. Regression test: `a_broken_settings_file_is_reported_and_not_saved_over`.

### 7. Imported `span` anchors were never validated (`crates/rain-comments/src/import.rs:39-75`, `notes.rs:57-58`, `ui.rs:67`)
**Rule:** validate untrusted input at the boundary.
**Problem:** only `position` was checked. The anchor is built from `span` whenever one is present, so HTTP and CLI imports could store a span with any of these:
- `startLine: 0`
- a start after its end
- `u32::MAX` columns

The label code then ran `start_col + 1`, which panics in debug builds. The thread never got a snapshot and never re-anchored.
**Fix (done):** `check_span`, used by both comment imports and line notes, requires `startLine >= 1` and start ≤ end, and keeps values within `u32`. The label arithmetic is done in `u64`. `span` is Rain-only, so difit's validation texts are unchanged. Tests extend `import_validation_matches_difit_errors` and add `span_labels_never_overflow`.

### 8. "Mark group viewed" synced the whole session once per file (`crates/rain-app/src/app/nav.rs:181`, `view/sidebar.rs:212`)
**Rule:** performance on the UI thread.
**Problem:** each file went through `mutate` and then `sync_session`. Each sync cloned the session and re-scanned `viewed` for every key, so a 1,000-file group cost about 10⁹ comparisons and 1,000 session clones.
**Fix (done):** `persist_viewed_keys` applies the whole batch in one `mutate`. The end state is the same.

### 9. An overview pushed over HTTP left Chapters mode showing the old chapter's files (`crates/rain-app/src/app/session.rs:40-48`)
**Rule:** correctness (stale derived state).
**Problem:** `retag` rebuilt `chapters` but not `chapter_view`. The header showed the new chapter, while the diff, ruler and Space walk still used the old chapter's file list.
**Fix (done):** re-enter the current chapter after `retag`. Regression test: `a_pushed_overview_refreshes_the_chapter_on_screen`, which failed before the fix (`[0, 5, 1]` vs `[3, 4]`).

### 10. `set_texts` deep-cloned a shared file once per view (`crates/rain-diffview/src/state.rs:547-549`)
**Rule:** clones in hot paths.
**Problem:** chapter and filter subsets share each file's `Arc<FileDiff>` with the main diff. Calling `set_texts` on each of them ran `Arc::make_mut` while the count was above 1. That copied the whole line arena on the UI thread once per view, and each view then kept its own copy.
**Fix (done):** add `DiffState::share_file`, which shares one tail with `set_texts`. The app's `set_texts_everywhere` hands the subsets the main diff's new `Arc`, so the file is copied at most once. The `a_filtered_view_and_the_full_diff_open_the_file_together` test now asserts `Arc::ptr_eq`.

### 11. Unified mode built every annotation widget twice per frame (`crates/rain-diffview/src/view.rs:617-644`, vendored and adapted)
**Rule:** performance.
**Problem:** `notes(Old)` and `notes(New)` were built only to test whether they were empty, then rebuilt for the unified column. Annotated rows skip the row memo, so every visible thread or composer was built twice per frame.
**Fix (done):** test for annotations with `annotations_at(..).next()`, and build `notes(..)` only in the Split arm. The output is identical.

### 12. Session files were not flushed before the rename (`crates/rain-comments/src/store.rs:102`)
**Rule:** atomic writes need durable data.
**Problem:** NTFS journals the rename but not the file data. After a power cut, `<id>.json` could come back empty, and the CLI then refuses to start. A failed write also left a stray `.tmp` file.
**Fix (done):** create the file, `write_all`, then `sync_all` before the rename, and remove the temp file on failure.

## Consider

Fixed (cheap and safe):
- **13. A loader-thread panic left the window on "Loading" forever** (`crates/rain-app/src/load.rs:435`). Wrapped in `catch_unwind`, so the panic message is shown as the load error. `Shared::lock` already recovers from poisoning.
- **14. A literal `{line}` in a file name was replaced too** (`app/editor.rs:128`). `{line}` is now substituted before `{path}`. Test added.
- **15. Self-repo containment took the first match, not the longest** (`crates/rain-overview/src/coverage.rs:129`). With `acme-app` and `acme-app-admin`, folder `…-admin-wt-1` resolved to `app`. It now takes the longest contained `dir`. Test added.
- **16. The word-diff cap disagreed with the docs** (`rain-diffview/src/state.rs:160` vs `myers.rs:206`). The 4 KiB constant could never take effect because `words` stops at 1000 bytes. The constant is now 1000 and `VENDORED.md` says so. No behaviour change.

Skipped, with reasons:
- **17. `settings.window` is read but never written** (`rain-app/src/lib.rs:321`). Persisting the window size is a product decision, not a bug fix.
- **18. `Diagram` clones the whole `Layout` on every build** (`rain-diagram/src/draw.rs:255`). Fixing it means changing `Diagram::new` to take `Rc<Layout>` across crates and app state. That is a perf-only refactor with no measurement behind it.
- **19. Syntect fallback grammars highlight on the UI thread** (`rain-diffview/src/state.rs:1366`). The only real fix is off-thread prefetch or paint-then-repaint, which is a design change. Not cheap. *Since done:* these languages now draw plain first and a worker colours them (`docs/decisions.md`, entry 15).
- **20. Autosave holds the session lock through up to about 2.1 s of rename retries** (`rain-comments/src/store.rs:157-163`). This is deliberate: it guarantees an older snapshot never lands after a newer one. Splitting the locks adds moving parts, and nothing has been observed to hitch. *Since changed:* the session is serialised under the lock and written after it is let go; numbered snapshots keep the order (`docs/decisions.md`, entry 5).
- **21. One OS thread per HTTP request; `/api/events` holds one for up to 60 s** (`rain-server/src/lib.rs:76-82`). Only local, non-browser clients can reach it. A pool is not worth the complexity.
- **22. A failed save is logged, but the route still answers 200** (`rain-server/src/lib.rs:146-152`). The change stays in memory and is saved again on the next change and at exit. Changing the reply would alter the difit-compatible contract in `docs/compat.md`.
- **23. Poisoned locks are recovered with `into_inner`** (`rain-core/src/session.rs:113`). This is documented and deliberate: one panicking request must not take the UI down.
- **24. `u32` text-arena offsets wrap past 4 GiB of diff text in one file** (`rain-core/src/diff.rs:166`). Only a single file diff over 4 GiB could trigger it, which is not realistic.
- **25. Generated-file checks compare extensions case-sensitively** (`rain-git/src/generated.rs:192-216`). This is a byte-for-byte port of difit's `generated-file-check.ts`. Changing it would make the two disagree.
- **26. The CLI reports a read timeout as "Cannot connect"** (`rain-cli/src/client.rs:50`). Cosmetic, and the text matches difit's error.
- **27. `Settings::save` is not atomic** (`rain-app/src/settings.rs:92`). Low value: the file is small and rewritten whole on the next change, and #6 now protects a broken file from being overwritten. *Since done:* `Settings::save` and `save_merged` go through `write_atomic`, the same temp-file, flush and rename as the session.

## Checked and not a problem
- **Server:**
  - Any `Origin` header gets a 403. That stops browser CSRF, and cross-origin DELETE and PUT preflights.
  - `Host` must be exactly `localhost|127.0.0.1|[::1]:<port>`, which stops DNS rebinding.
  - POST and PUT must be `application/json`, so a `<form>` cannot post.
  - The body is limited both on `Content-Length` and with `take(limit + 1)`.
  - No route touches a path taken from the request. The session file path is fixed when the server starts.
  - Malformed JSON returns 400. No handler panics on input.
- **Git argument injection:**
  - `rev_parse` rejects a leading `-` and passes `--end-of-options`.
  - Every other revision is a resolved commit id, `--cached`, or a hex-checked id (`line_map`).
  - Paths always follow `--`.
  - `blob` checks `-` and runs `worktree_path`, which rejects `..`, absolute paths and prefixes.
  - `-c core.quotePath=true` and the other pinned settings stop user config from changing the parsed output.
- **Session files:** `sanitize` keeps only `[A-Za-z0-9._-]` and trims dots, so `..`, separators, drive letters and ADS can't escape. `CON.json` and similar names write fine on Windows 11 (tested).
- **Untrusted anchors and goto in the UI:**
  - Threads are placed only when both ends resolve.
  - `byte_of_col` clamps by chars.
  - `anchor_text` rejects `0` and `last < first`.
  - Goto lines fall back to the file.
- **Data-dir migration:** the old folder is copied into a staging folder, then renamed into place. A race with another process doing the same is handled.

## What's already good
- **Server defences are layered** (Origin, then Host, then Content-Type, then size), and each layer is tested in `tests/routes.rs`.
- **Saving is ordered:** the HTTP routes and autosave both save under the session lock with tmp-and-rename, so an older snapshot can never land last. (Now: serialised under the lock, written after it, in snapshot order.)
- **The streaming git driver** serves stdin and stderr on threads, so pipes can't deadlock. It kills the child on drop. The parser is fuzzed (`hostile_and_fuzzed_patches_never_panic_or_blow_up`).
- **The 1M-row diff state** uses 16-byte rows, `row_of` by arithmetic or binary search, and UTF-8-safe byte/column round trips throughout.

## Lints
The workspace has no `[lints]` table. The BeautifulRust config (`lints/lints.toml` and `lints/clippy.toml`, https://github.com/morganlinton/BeautifulRust) would have flagged:
- the raw `files()[f]` indexing in #2 (`clippy::indexing_slicing`)
- the `unwrap_or_default()` that swallowed the parse errors in #3 and #6

It would not have caught the logic bugs (#1, #4, #9).
