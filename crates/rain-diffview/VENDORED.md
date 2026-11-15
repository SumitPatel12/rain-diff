# Vendored: rune-kit `diff`

Source: `https://github.com/anomalyco/rune`, rev `781e2b6d87cd5f32d6f4365956fad4baaf24ee50`,
crate `rune-kit`. Licence: MIT, Copyright (c) 2026 Anomaly; the full notice is in `LICENSE-rune`.

| upstream | here |
|---|---|
| `src/diff/myers.rs` | `src/myers.rs`, unchanged except `Op`, `diff` and `words` made `pub` (doc-hidden) for the test helper |
| `src/diff/model.rs` | split: hunk bounds, language, header, whole-text line index and change blocks in `src/model.rs`; the patch parser and two-text differ moved to `tests/common/patch.rs` (test and example helper only) |
| `src/diff/state.rs` | `src/state.rs`, rewritten for 1M+ rows (below) |
| `src/diff/view.rs` | `src/view.rs`, adapted (below) |
| `src/diff/mod.rs` | `src/lib.rs` |
| `examples/diff.rs` | `examples/diffview.rs`, new content on the Rain fixtures |
| `tests/diff.rs` | `tests/diff.rs`, every case ported to the core model, plus new cases |

## Changes

Model
- The private `Line`/`Hunk`/`FileDiff` are gone; the view reads `rain_core::diff::{FileDiff, Hunk, LineRec, LineKind, Side, FileStatus}` (one text arena per file, 24-byte line records, line numbers 0 for none).
- `FileDiff::from_texts`, `between` and `parse` are no longer library API; `tests/common/patch.rs` keeps them, building core files through `FileDiffBuilder`. Similarity and file modes are not in the core model, so headers no longer show a mode change.
- Word changes are no longer computed at load (`pair_words` removed). `DiffState::word_changes(file, line)` computes them when a row is built, from change blocks (`model::blocks`, one 12-byte record per removed/added run); same rule as upstream (pairs in order, none when a block pairs more than 128 lines), and upstream's 1000-byte per-line cap checked before the line pair is read.
- Whole texts are `Option<Arc<str>>` on the core file; `LineIndex` indexes them when present. `DiffState::set_texts` adds or drops them later (expansion after a `git show`), `collapse_expanded` folds every opened range.
- `columns` comes from `FileDiff::max_columns` (tracked by the builder) instead of walking every char at load; whole texts are measured only when present.

State (1M rows)
- `Row` is 16 bytes (asserted): `Line { f, i }` and `Pair { f, del, add }` index the file's flat line list, so the hunk index is gone.
- `keys: Vec<String>` removed: `key_of(row)` formats the same `df{f}.c{old}` / `o{old}` / `n{new}` keys on demand; the view passes `virtual_rows_version(keys_rev)` so a commit doesn't call it per row.
- `row_of: HashMap` (two entries per line) removed: `row_of(file, side, line)` finds the hunk by binary search, the line by binary search over the side's line indexes, then the row arithmetically (unified, unchanged ranges) or by binary search over the hunk's rows (split; rows ascend by their smallest line index). A test checks it against the old hash map on `fixtures/m.patch` in both modes, collapsed and expanded.
- `side_lines` per hunk (a `Vec` per hunk per side) replaced by one ascending `Vec<u32>` per file per side.
- `rebuild()` reuses the rows vector and fills per-hunk tables (`gap_rows`, `hunk_line_rows`, `hunk_rows`); no allocation per line. The reader's place survives a rebuild by (file, side, line) instead of a linear search for the old top key.
- Hunk navigation uses binary search over `hunk_rows`.
- Keys: `j`/`k` move a new line cursor (was next/previous hunk), `n`/`p` next/previous hunk, `]`/`[` files, Escape clears the line selection, then the text-span selection, then the cursor. The cursor is public API (`cursor`, `set_cursor`, `cursor_down/up`, `comment_at_cursor`); `scroll_to_line`, `next_hunk` and `next_file` move it.
- Annotations take a `rain_core::comments::Anchor` (line, range or span) and sit under the anchor's last line (`Annotation::line` for the old one-line form). Range and span anchors are indexed per file for the gutter mark and span tint.
- Text-span selection: `span_from_texts` maps a Rune `TextSelection` (row key and side in the element path, byte range in the drawn text) back to (file, side, line, char column); `span_selection()` keeps the latest.
- `repo_ids`/`display_path` prefix header paths with a repo id; `set_collapsed_where`, `into_files`, `row_key`, `row_count`, `heap_bytes` added.
- Round 2 row API for an overview ruler: `visible_rows`, `file_rows`, `hunk_row(file, hunk)`, `row_file(row)`, `row_of_line(file, LineRef)` (public `row_of`), `annotation_rows()`; `row_y(row)` / `rows_height()` estimate a row's top from the fixed row heights, by binary search over a per-rebuild `tall_rows` table (headers, notes, gap bars: O(files + hunks)).
- Whole file: `FileUi.whole`; `whole`, `set_whole`, `set_whole_where` (one rebuild), `needs_texts`, `opened`/`set_opened` (carry a file's whole and revealed state to another diff over the same files). Whole opens every range of known length (the trailing one once texts exist); `set_texts` re-applies it, `collapse_expanded` ends it.

View
- Row keys come from `key_of` (`Cow::Owned`), not a stored vector.
- `cells()` reads the core line and asks the state for word changes; the row memo now includes the cursor, range marks and span marks.
- Range/span comments draw a 3 px bar beside the line numbers on every covered line; span comments tint the selected text with `TextHighlight` (paint only, glyphs don't move). The highlight ranges go through the same tab/zero-width-space shift as the syntax spans.
- `on_selection_change` on the list turns a finished drag into a `SpanSelection` (`on_span_select`).
- `header_annotation(|s, file| Option<El>)` slot under each in-list file header.
- `calm_added_files(true)`: files of 200+ lines that are all added keep the bar, lose the wash.
- The cursor's row draws its bar in the accent colour.
- `palette::tint` (crate-private upstream) copied into `view.rs`.
- `scroll_live(Live<[f32; 4]>)` and `on_scroll_offset(Fn(ScrollPos))` on the viewport (one `El::on_scroll_offset` handler feeding both); `SCROLL_KEY = "diff/scroll"` exported for `Tx::scroll_to_y`.
- Note rows: "Contents unavailable" for `contents_unavailable`; no mode-change note. Headers show "Generated" for generated files.

Round 3
- Per-file layout: `Layout { Single, Split }` from `DiffState::layout(file)`, decided from the file and the mode (no stored table): in split mode a one-sided file (added, deleted, binary, rename-only, contents unavailable) draws one full-width column with one number column (`Draw::One(side)` through `View::unified`), no empty half. Its rows don't change (they never paired).
- Narrow split: `set_min_split_columns(n)` (0 = off, the default); the view's root reports the code columns one side gets (`on_layout_tx` → `set_columns_per_side`, `live_only` unless the layout flips); below `n` every file draws unified (rows rebuilt without pairs) and `layout_reason()` says why. `row_of` and the row builder ask `pairs()` instead of the mode.
- Spotlight: `Spot { file, side, lines, caption, step }`, `set_spotlight`, `spotlight_rows`, `dimmed(row)`. Rows outside draw at 45 % opacity (the dim flag is in each row's memo version: paint only, no row changes). One outline element keyed `spot` in an absolute, clipped layer over the list, placed from layout reads of the first and last spot rows (row estimates until they are built) and shifted with the scroll by a `Live<[f32; 2]>` set in the viewport's `on_scroll_offset` (no commit per frame); a step/caption chip sits on its top-right edge.
- `scroll_to_range(file, side, lines, Align, tx)` opens what hides the range and scrolls its first row a fifth of the way down the viewport (`Align::Third`), below the header (`Top`) or to the middle (`Center`). The viewport's `on_scroll_offset` is always attached now and records offset and height (`scroll_offset()`, `viewport_height()`).
- Stable hunk ids: `hunk_id(file, hunk)` = 16 hex digits of blake3(path, first and last changed line); `set_files` carries ids from the files it replaces by path (or `old_path` for a rename): the same own id keeps it, else old-side range intersection (one: inherit; several: the biggest's; none: own id), bigger new hunks claiming first so a split hunk's smaller part gets its own. `hunk_by_id`, `hunk_hash(file, hunk)` (every line's kind and text) for hunk-level viewed, `visible_hunk()`.
- New dependency: `blake3` (workspace).

## Clippy allows

- `#[allow(clippy::type_complexity)]` on `DiffView` (callback fields), as upstream.
- `#[allow(clippy::too_many_arguments)]` on `View::number` and `View::split`, as upstream, and on `View::unified` (round 3 added its `one` side).
- `#[allow(clippy::single_range_in_vec_init)]` on one view unit test, as upstream.
