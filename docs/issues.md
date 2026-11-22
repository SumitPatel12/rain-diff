# Issues

Hacks and doubts found while deleting comments: each comment that excused an implementation was
deleted and the work it stood for is written down here instead. Line numbers are as of the commit
that added the entry. Close an entry by deleting it in the commit that fixes it.

Per entry: where, what the deleted comment admitted, what a proper fix looks like, rough size.

## 1. The 20,000-line collapse rests on a cost the rune fork may have removed

- **Where:** `crates/rain-app/src/load.rs:16` (`LARGE_FILE_LINES`), used by the first fold, chapters,
  filters, the since-round lens and expand-all (`app/folds.rs`, `app/chapters.rs`, `app/nav.rs`,
  `app/board.rs`, `app/review.rs`, `app/wholefile.rs`).
- **The comment said:** files over the limit start collapsed because "Rune keys every row of a row
  change, so a 500k-line snapshot costs seconds of first frame until it is opened".
- **Why it is doubtful:** the workspace now pins a rune fork whose one extra commit makes a virtual
  list key rows on demand (`Cargo.toml`, `virtual_row_index` in `rain-diffview`), and
  `rain-diffview::row_keys_are_built_for_the_rows_near_the_viewport_not_for_every_row` holds it.
  Whether a row change still costs a key per row (`rekeys()` in `rain-diffview/src/state.rs` counts
  "layouts that key every row again") is not settled by either comment.
- **Fix:** measure a first frame on a 500k-line added file with the collapse off. If it is within the
  frame budget in `docs/perf.md`, drop the collapse for that reason (keep it for expand-all if the
  texts are the cost) and say why in one line; if not, record the measured cost as the reason.
- **Size:** small (a measurement and a constant), medium if expand-all needs its own limit.

## 2. Chapter navigation left behind after chapters stopped narrowing the reader

- **Where:** `crates/rain-app/src/app/chapters.rs:46` (`in_chapter`), its one caller at
  `crates/rain-app/src/app/session.rs:49`, `step_chapter` in the same file, and
  `Command::NextChapter`/`PrevChapter` in `crates/rain-app/src/app/mod.rs` (`run`).
- **The comment said:** "Chapters no longer narrow the reader; kept for callers that refresh one."
  `in_chapter` always returns `false`, so the branch in `sync_session` never runs. `step_chapter`
  has no caller: the chapter commands run `step_section`. The `chapter` field's doc ("the chapter
  `J`/`K` last moved to") was stale for the same reason and was deleted.
- **Fix:** delete `in_chapter` and the dead branch, delete `step_chapter`, and decide whether
  `NextChapter`/`PrevChapter` stay as aliases of the section commands (keymap compatibility for
  `settings.json` overrides) or go. Keep `goto_chapter` (the palette's chapter rows use it).
- **Size:** small.

## 3. Round-2 commands kept in the command set "until the round-3 shell replaces it"

- **Where:** `crates/rain-core/src/command.rs:65` (`NextChapter`, `PrevChapter`,
  `ToggleChaptersQueue`, `ToggleInspector`, `Overview`, `FocusTree`), dispatched in
  `crates/rain-app/src/app/mod.rs` (`run`).
- **The comment said:** these are "round-2 commands with no chord and no place in `Command::ALL`;
  the round-2 shell still dispatches them until the round-3 shell replaces it". The round-3 shell is
  the one that ships; the chapter pair now runs the section commands (entry 2).
- **Fix:** for each variant, find whether anything still sends it (`Command::from_id` only accepts
  ids a key can run, so a `settings.json` override cannot name them). Delete the ones nothing sends;
  map any kept one to its current command and say why in one line.
- **Size:** small.
