# Performance budgets

Rain should feel instant and stay small on a low-spec machine: 4 GB of RAM and a GPU. Every
budget below is a limit a change must stay inside, not a record of how fast Rain happens to be.

## Three lanes

1. **Counts, in every test run.** Runs of the view, rows built, layouts, matching passes,
   allocations and bytes. A count repeats exactly, so these are ordinary tests and they fail the
   build. How they are counted: [perf-counts.md](perf-counts.md).
2. **Clocks, alone, in a release build.** Times move by 10% or more between two runs of the same
   code, so no normal test asserts one. The `#[ignore]` tests print them; run them with nothing
   else building or testing, and compare against the table.
3. **By hand, before a release.** What no test can see: the time from a key to the screen in a
   real window, and the memory of the whole process.

Each budget has a **whole-frame** limit, which is what the user feels, and where it helps a
**work-only** limit on one part of the frame, which says early which part got dearer. A count
budget only goes down: lower it in the commit that makes the work cheaper. Raising one needs the
reason in the commit message.

## Budgets

Clock budgets are for a release build on the development machine, in a 1600x1000 window. Sizes:
**sample** is the small review in `crates/rain-app/fixtures/story.patch`; **L** is 300 files and
60,000 diff lines; **XL** is 302 files and 1.36 million diff lines (`cargo xtask fixtures --tier l`
or `xl`). In "Last measured", **before** is the build before this batch of performance work and
**this batch** is that work; a debug number is from the test build, a release number from
`--release`.

| What | Workload | Whole-frame budget | Work-only budget | Enforced by | Last measured |
|---|---|---|---|---|---|
| Cursor key (`j`) | Counts: sample. Clock: `j` held 300 times in a 20,000-line file | 1 view run, 2,925 allocations. p95 4 ms | 2 line rows built | `rain-app::one_cursor_down_stays_within_its_counts`, `rain-app::a_cursor_move_builds_the_two_rows_it_touches_with_threads_on_screen`. Clock by hand: `holding_keys_frame_cost` | Debug, this batch: 1 view run, 2 rows, 2,869 allocations. Release, before: p50 2.4 to 2.6 ms, p95 2.9 to 3.4 ms, 2.87 rows and 2,685 allocations a key. Release, this batch: p50 2.3 to 2.4 ms, p95 2.7 to 2.9 ms (one run of five reached 4.3 ms), 2.87 rows and 2,310 allocations a key |
| Held cursor key | Sample, 10 presses | Every later press costs what the first did: same view runs, allocations within 2% | Same rows built as the first press | `rain-app::a_held_cursor_key_costs_no_more_on_its_tenth_press_than_on_its_first` | Debug, this batch: 1 view run and 2 rows on every press; 2,673 to 2,870 allocations, the first press the dearest |
| Idle | Sample, 60 frames with no input | 0 view runs, 246 allocations | 0 rows built | `rain-app::an_idle_frame_rebuilds_nothing` | Debug, this batch: 0 view runs, 241 allocations. Release, before: 0 frames asked for in 240 |
| Space (mark viewed, go to the next file) | Counts: sample. Clock: XL | 2 view runs, 12,355 allocations. 16 ms | 19 line rows, 1 layout, 1 re-key | `rain-app::space_marks_viewed_and_goes_next_within_its_counts`. Clock by hand: `xl_launch_to_first_frame` prints it | Debug, this batch: 2 view runs, 19 rows, 11,787 allocations. Release, before: 48 ms on XL; on L 60 ms, 5 view runs, 227 rows, 359,105 allocations. Release, this batch: 11 ms on XL as `xl_launch_to_first_frame` prints it (11 to 12 over five runs). Pressed 20 times in a row on XL the median is 16.4 ms (15.8 to 17.5) and p95 18.9 ms, **over the 16 ms budget**. On L 16.8 ms, 4 view runs, 122 rows, 36,253 allocations |
| Find, a typed letter | Counts: sample. Clock: L for the frame, XL for the scan | 1 view run, 4,783 allocations. 8 ms | 14 line rows. A scan of XL's 44 MB of text: 5 ms | `rain-app::a_find_letter_and_enter_stay_within_their_counts`, `rain-app::a_find_letter_builds_only_the_rows_whose_marks_change`. Clock by hand; no test prints it yet | Debug, this batch: 1 view run, 14 rows, 4,691 allocations. Release, before: 7.9 ms a frame with 175 to 181 rows built on L; scan 3.2 to 3.5 ms mean, 4.6 ms worst on XL. Release, this batch: 4.8 ms a frame (4.5 to 5.8 over five runs) with 22 to 46 rows built on L, 3.9 ms and no rows when nothing matches; scan 3.0 to 3.4 ms mean, 4.5 ms worst on XL (4.9 ms in one run of five) |
| Find, Enter to the next match | Counts: sample. Clock: L | 1 view run, 3,634 allocations. 8 ms | 2 line rows | `rain-app::a_find_letter_and_enter_stay_within_their_counts`, `rain-app::moving_the_current_match_builds_the_row_it_leaves_and_the_row_it_reaches`. Clock by hand; no test prints it yet | Debug, this batch: 1 view run, 2 rows, 3,564 allocations. Release, before: 19 ms, 3 view runs, about 310 rows on L. Release, this batch: 5.1 ms (5.0 to 6.2 over five runs), 1 view run, 9 rows on L. The first Enter after typing, which also jumps to the first match, takes 16 ms (14 to 23) and 5 view runs: **over the 8 ms budget** |
| File picker, a typed letter | Counts: sample. Clock: 300 and 5,000 files | 1 view run, 4,566 allocations. 8 ms at 5,000 files | 1 matching pass, 0 diff rows, 0 layouts. The pass: 0.2 ms at 300 files, 1 ms at 5,000 | `rain-app::a_picker_letter_matches_once_and_builds_no_diff_row`, `rain-app::the_picker_matches_once_per_query_not_per_key_or_frame`. Clock by hand; no test prints it yet | Debug, this batch: 1 view run, 1 pass, 0 rows, 4,378 allocations. Release, before: frame 5.5 ms at 300 and 14.6 ms at 5,000; matching 0.47 and 8.2 ms. Release, start of this batch (new matcher): frame 5.1 and 5.7 ms; pass 0.017 and 0.12 ms. Release, this batch: frame 3.7 and 4.9 ms; pass 0.018 and 0.13 ms |
| File picker, an arrow key | Sample | 1 view run, 5,975 allocations | 0 matching passes, 0 diff rows | Same two tests | Debug, this batch: 1 view run, 0 passes, 5,760 allocations. Release, start of this batch: frame 4.3 ms at 300 files, 5.2 ms at 5,000 |
| Command palette, a typed letter | Counts: sample. Clock: 300 and 5,000 entries | 1 view run, 4,147 allocations. 8 ms at 5,000 entries | 1 matching pass, 0 diff rows, 0 layouts | `rain-app::a_palette_letter_matches_once_and_builds_no_diff_row`, `rain-app::the_palette_matches_and_marks_once_per_query_not_per_key_or_frame`. Clock by hand; no test prints it yet | Debug, this batch: 1 view run, 1 pass, 0 rows, 3,968 allocations. Release, before: frame 4.5 ms at 300 and 5.1 ms at 5,000. Release, start of this batch: 4.5 and 5.0 ms. Release, this batch: 3.5 and 4.1 ms |
| Command palette, an arrow key | Sample | 1 view run, 3,346 allocations | 0 matching passes, 0 diff rows | Same two tests | Debug, this batch: 1 view run, 0 passes, 3,182 allocations. Release, start of this batch: frame 3.8 ms at 300 entries, 4.7 ms at 5,000 |
| Scrolling inside one file | Counts: 60 wheel frames of 60 px in a 2,000-line file. Clock: 1,000 wheel frames in XL's largest file | 15 view runs and 47,292 allocations in the 60 frames. Frame with the GPU submit: p95 8.3 ms | 0 layouts, 0 re-keys, 180 line rows (the 3 that come into view each frame). Diff view alone: p50 1 ms, p95 3 ms, max 8 ms | `rain-app::wheel_frames_inside_one_file_lay_nothing_out_and_build_only_the_rows_that_come_into_view`, `rain-app::wheel_scrolling_runs_no_view_for_the_ruler`. Clock by hand: `xl_state_and_scroll`, and a headless scroll script | Debug, this batch: 15 view runs, 180 rows, 45,825 allocations. Release, before: diff view alone p50 0.65, p95 3.08, max 6.94 ms, 200 view runs in 1,000 frames; with the GPU submit p50 5.06, p95 7.72 ms. Release, this batch: diff view alone p50 0.76, p95 4.10, max 6.25 ms (medians of five runs), 200 view runs in 1,000 frames. **p95 is over its 3 ms**: it ran from 2.71 to 5.9 ms and was over in three runs of five; the build before, measured in the same session, gave 2.95 ms (2.8 to 6.28), over in two of five. Max was over 8 ms in two runs of five on both builds. With the GPU submit p50 4.9, p95 7.6 ms |
| First frame of a very large diff | Counts: 200,000 rows. Clock: XL with every file open (the app folds files over 20,000 lines when it opens) | In the app, data to first frame on XL: 200 ms | Under 400 row keys built in the first frame, under 1,500 for a fold, an unfold or a jump to the end. Diff view alone, first frame on XL: 100 ms | `rain-diffview::row_keys_are_built_for_the_rows_near_the_viewport_not_for_every_row`. Clock by hand: `xl_row_keys`, `xl_launch_to_first_frame` | Release, before: 976 to 1,015 ms, a key for each of 1.36 million rows; in the app 108 ms. Release, this batch: 37 ms (35 to 42 over five runs) and 370 keys. In the app: 97 ms (96 to 128) |
| A very long line | One line of 200,000 characters | | A row build and a cursor step cost what the first `LINE_CAP` bytes cost, in bytes and allocations | `rain-diffview::a_very_long_line_costs_what_its_drawn_part_costs`. Table by hand: `table_of_costs_by_line_length` | Debug, one step on a line of 2 million characters: 1.1 to 4.1 s before, 4 to 26 ms this batch |
| Expand all files | 40 files | | 5 layouts and 5 re-keys, not one per file | `rain-app::shift_e_lays_the_rows_out_once_per_batch_of_files_not_once_per_file` | Debug, this batch: 4 layouts for 40 files, where it was 40 |
| Memory: the diff's own state | Counts: 200,000 diff lines. Whole: XL | 60 MB beyond the files on XL | 44 bytes a diff line; a line record 24 bytes; a row 16 bytes | `rain-diffview::the_state_of_a_large_diff_stays_within_its_bytes_per_line`, `rain-core::line_rec_is_at_most_24_bytes`, `rain-diffview::a_row_is_sixteen_bytes`. The 60 MB by hand: `xl_state_and_scroll` asserts it | Debug, this batch: 20.1 bytes a diff line. Release, before: 26.6 MB on XL. Release, this batch: 26.6 MB on XL |
| Memory: the whole process | L and XL, opened and then scrolled | Tracked and reported, not enforced | | By hand | Release, before, peak working set: L 216 MB opened and 250 MB scrolled; XL 311 MB and 346 MB (no window); 269 MB for L and 364 MB for XL in a real window. Live heap of the diff view after XL's first frame, release: 312 MB before, 185 MB this batch. Release, this batch, peak working set: L 209 MB opened and 241 MB scrolled; XL 304 MB and 338 MB (no window); 251 MB for L and 346 MB for XL in a real window |
| Key to screen | A real window | Tracked and reported, not enforced | | By hand | Not measured yet |

## How to measure

Run these one at a time, with no other build or test running. Each prints its numbers; a few
also assert. `pwsh scripts/gates.ps1 -Perf` runs them all after the normal gate.

Generate the large diffs first (they are not checked in):

```
cargo xtask fixtures --tier l
cargo xtask fixtures --tier xl
```

They land in `fixtures/gen/<tier>/`. `RAIN_L_PATCH` and `RAIN_XL_PATCH` point the tests at a
patch somewhere else.

| Measures | Command |
|---|---|
| Diff state on XL: build time, memory, scroll frames, jump to the last row (`xl_state_and_scroll`); row keys built and live heap on XL (`xl_row_keys`); spotlight on L | `cargo test -p rain-diffview --release --test perf -- --ignored --nocapture --test-threads=1` |
| Row keys alone | `cargo test -p rain-diffview --release --test perf xl_row_keys -- --ignored --nocapture` |
| Cost of a row build and a cursor step as a line grows | `cargo test -p rain-diffview --release --test long_lines table_of_costs_by_line_length -- --ignored --nocapture` |
| Held keys in the app: `j`, `k`, `l`, `h`, `w`, `b` | `cargo test -p rain-app --release holding_keys_frame_cost -- --ignored --nocapture` |
| XL in the app: load, first frame, layout switch, jump, Space | `cargo test -p rain-app --release xl_launch_to_first_frame -- --ignored --nocapture` |
| Whole file and Expand all, with `RAIN_WHOLE_REPO` set to `fixtures/gen/<tier>/repo` | `cargo test -p rain-app --release whole_file_timings -- --ignored --nocapture` |
| Git: `git diff`, parse and resolve on XL and L, peak memory | `cargo test -p rain-git --release --test perf perf_xl_then_l -- --ignored --nocapture` |

By hand, before a release:

- **Whole-process memory.** Open a large diff with no window and read the process's peak working
  set from the operating system when it exits:
  `Rain.exe --fixture <patch> --session <fresh name> --clean --scene session --first-pixels --size 1600x1000 --scale 1`.
  Do the same with a scroll script (`--script <file> --frames 620 --fps 120`) for the figure
  after scrolling.
- **Start.** Set `RAIN_TIMINGS=1` and open a real window: it prints the load time and the time
  from data to the first frame.
- **Key to screen.** Hold a key in a real window on the slowest machine at hand and watch for a
  frame that comes late. No tool measures this yet.
