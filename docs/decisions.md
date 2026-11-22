# Decisions

The choices that shape Rain, each with the reason, what it costs, and the test or line that holds
it in place. A decision is current while its pin holds: `crates/rain-cli/tests/decisions_doc.rs`
fails when a pinned test is renamed or removed, or when a pinned line no longer has the quoted text.

Rules for this page:

- One entry per decision: what, why (and what was turned down), the limit, the pin.
- A pin is `` `crate::test_name` `` or `` `path:line` has `text` ``. Prefer a test. Use a line only
  where no test can say it.
- Name a constant instead of repeating its value. The value lives in the code.
- Details stay where they are (`docs/compat.md`, `docs/agent.md`, `crates/rain-diffview/VENDORED.md`);
  this page links to them.

## 1. One native process; the window thread owns the view

**Decision.** Rain is one executable that draws its own window through the `rune` crate, pinned to
one upstream revision. The window thread owns everything on screen. Git work runs on a loader
thread and each HTTP request on its own thread. They reach the window only through one shared
object: the session behind a lock, a few small mailboxes, and a wake call.

**Why.** A browser UI with a local server adds a browser tab's memory to every review and puts the
app inside a page that other pages can try to talk to. Splitting the app into a core process and a
front end adds a protocol and a second copy of state to keep in step. One process has neither.

**Limit.** The UI crate is not ours to change, and there is no remote review. Requests are capped
at `MAX_IN_FLIGHT`; past it a client gets a 503 and must retry.

**Pinned by:**
- `Cargo.toml:30` has `rune = { git`
- `crates/rain-app/src/lib.rs:4` has `the loader thread does the git work`
- `crates/rain-core/src/session.rs:441` has `The session behind a mutex`
- `rain-server::requests_past_the_in_flight_cap_get_a_503_until_a_slot_frees`
- `rain-server::health_answers_while_long_polls_fill_every_request_slot`

## 2. Nothing leaves the machine

**Decision.** Rain makes no network call: no telemetry, no update check, no fetched assets. The
server listens on loopback only. The only HTTP client is the CLI talking to a running window on
`localhost`, built without TLS and with proxies off.

**Why.** A review holds unreleased code. A rule that rests on review alone erodes one dependency
at a time, so the rule is a test: the resolved dependencies may hold no network client, TLS stack
or updater, and first-party code may name a socket only in a short list of files.

**Limit.** No auto-update and no crash reports. The one way out is the command the user configures
to answer questions (entry 15), which Rain does not control.

**Pinned by:**
- `rain-server::no_resolved_dependency_can_call_out`
- `rain-server::only_the_listed_files_touch_sockets_and_only_on_loopback`
- `rain-server::the_server_answers_on_loopback_and_not_on_the_machines_own_address`
- `rain-server::every_listener_is_on_loopback`

## 3. The server refuses browsers; one token guards one privilege

**Decision.** A request with an `Origin` or `Sec-Fetch-*` header, a `Host` that is not the loopback
name with our port, or a write that is not JSON is refused. A per-window token guards only the one
write that can start a process: asking the answerer. Every other route stays open to local
processes.

**Why.** Comments read back from Rain become an agent's instructions, so a web page must never be
able to write one. Rain has no page of its own, so it can refuse every browser instead of trying to
tell its own page from a hostile one. A token on every route would break difit tooling, which sends
none.

**Limit.** Another user on a shared machine can read and write a session through the port. See
"Posture" in [compat.md](compat.md).

**Pinned by:**
- `rain-server::browser_shaped_requests_are_refused`
- `rain-server::new_routes_keep_the_browser_defences`
- `rain-server::only_a_caller_with_the_windows_token_starts_its_answerer`
- `rain-server::the_lock_file_keeps_the_token_and_the_lock_route_does_not_serve_it`

## 4. difit's contract is kept, with two deliberate breaks

**Decision.** difit's routes and JSON keep their shape, so tooling written for difit works. Two routes behave differently on purpose: `DELETE /api/comments/:id` resolves the thread and
keeps its history, and `POST /api/comments` merges by id and keeps every thread the body leaves out.

**Why.** An agent reads a filtered list (open threads, or one repo) and posts it back. With difit's
"replace the list" that read-then-write deletes every thread it did not see.

**Limit.** A client that expects a delete to remove, or a post to replace, finds the threads still
there under `?all=1`. Details in [compat.md](compat.md).

**Pinned by:**
- `rain-server::imports_read_back_byte_equal_to_difit`
- `rain-server::delete_resolves_keeps_history_and_hides_unless_all`
- `rain-server::post_comments_merges_on_a_stale_base_version`
- `rain-server::post_comments_without_threads_is_refused_and_keeps_every_thread`

## 5. A session is one JSON file, replaced whole and in order

**Decision.** Each save writes the whole session to a uniquely named temp file beside it, flushes
it to disk, and renames it over the old file. Saves are serialised under the session lock, written
after the lock is let go, and numbered, so an older one never lands after a newer one.
`settings.json` and the tour file are written the same way.

**Why.** Writing in place can leave half a file after a crash. A rename without a flush can leave
an empty file after a power cut. A database would be one more dependency for a file this small.

**Limit.** The whole session is rewritten on every save, which is fine at review sizes. A failed
save is logged and tried again on the next change; the HTTP reply does not report it.

**Pinned by:**
- `rain-comments::store_round_trips_every_kind_of_state`
- `rain-comments::an_older_snapshot_never_lands_after_a_newer_one`
- `rain-comments::autosave_writes_after_the_debounce_and_on_drop`
- `crates/rain-comments/src/store.rs:238` has `f.sync_all()`
- `crates/rain-app/src/settings.rs:345` has `write_atomic`

## 6. A file Rain cannot read is kept, never saved over

**Decision.** Each save keeps the session it replaces as one backup, made as a second name for the
old file so no bytes are copied. An unreadable session file is moved aside under a `.corrupt` name
and the backup opens in its place, with a notice. A broken `settings.json` is ignored for the run
and not written. A session file that fails with an I/O error turns saving off.

**Why.** Starting empty after a failed load and then autosaving is silent data loss. Refusing to
start would leave the user with no review and no explanation.

**Limit.** One generation of backup. On a file system without hard links there is none, and the
save still goes ahead.

**Pinned by:**
- `rain-comments::a_save_keeps_the_session_it_replaces_as_the_one_backup`
- `rain-comments::the_backup_is_a_second_name_for_the_old_file_not_a_second_write`
- `rain-comments::an_unreadable_session_file_falls_back_to_its_backup`
- `rain-app::an_unreadable_session_file_is_kept_aside_not_overwritten`
- `rain-app::an_unreadable_session_file_opens_the_save_before_it_and_says_so`
- `rain-app::a_session_file_that_cannot_be_read_is_never_autosaved_over`
- `rain-app::a_broken_settings_file_is_reported_and_not_saved_over`

## 7. One window writes a session; a second one is read-only

**Decision.** A window takes `<id>.lock` beside its session file. The lock appears whole and only
if absent. A lock is stale only when its port is closed, answers as something else, or is silent
and its process has exited; a busy window is not dead. A window that does not get the lock still
opens, never saves, and answers 423 to every write.

**Why.** Two writers of one whole-file session lose each other's changes. Refusing to open the
second window would stop the user from reading. Merging two live writers is a larger problem than
a review tool needs to solve.

**Limit.** The second window cannot comment. It names the port that can.

**Pinned by:**
- `rain-server::windows_racing_for_a_stale_lock_leave_exactly_one_owner`
- `rain-server::a_window_too_busy_to_answer_is_not_stale`
- `rain-server::a_window_that_finds_a_live_lock_serves_but_never_saves`
- `rain-server::a_server_that_cannot_take_its_lock_opens_read_only_and_says_why`
- `rain-app::a_read_only_window_never_saves_and_says_so`

## 8. Git through its command line, read-only

**Decision.** Rain runs the user's `git` and parses its output. No git library is linked. Every
call turns off optional locks and prompts and asks for messages in one language. Rain runs only
commands that read, so it never writes to the repository.

**Why.** A linked git library is a large native dependency with network code in it, and it can
disagree with the git the user runs. The command line is the same git, and its output format is
stable.

**Limit.** Git must be on `PATH`. Each call costs a process start, so calls are batched.

**Pinned by:**
- `rain-server::no_resolved_dependency_can_call_out`
- `rain-git::every_git_call_takes_no_optional_lock_and_never_prompts`
- `rain-git::diff_of_two_commits_matches_parsing_git_diff_output`

## 9. Diff settings are pinned, with the histogram algorithm

**Decision.** Every setting that changes what `git diff` prints is passed on each call, so the
user's git config cannot change it. The algorithm is `histogram`.

**Why.** Viewed marks and comment anchors are tied to hunks. If config could move hunks, the same
review would differ between machines. Histogram gives more readable hunks on moved and repeated
lines than git's default.

**Limit.** A user's own diff preferences are ignored inside Rain. Hunks can differ slightly from a
plain `git diff`, and from a session saved before the algorithm was pinned.

**Pinned by:**
- `rain-git::every_git_call_pins_the_diff_settings_and_drops_the_context_override`
- `rain-git::diffs_use_the_histogram_algorithm`
- `rain-git::hostile_git_config_does_not_change_what_rain_reads`

## 10. Commits are noticed by watching `.git`, never the working tree

**Decision.** A file watch covers `HEAD`, the refs and the index inside the git folder. It calls
back once a burst of changes has been quiet for `QUIET`, and at least every `CAP`.

**Why.** A recursive watch of a working tree costs memory per folder, runs into OS watch limits on
big repositories, and fires on every build output.

**Limit.** The watch alone does not see an edit to a file that is not committed or staged. Entry
11 covers that.

**Pinned by:**
- `rain-git::only_head_refs_and_the_index_wake_the_watch`
- `rain-git::the_watch_fires_on_a_commit_within_two_seconds`
- `crates/rain-git/src/watch.rs:77` has `RecursiveMode::NonRecursive`

## 11. Uncommitted edits are noticed by polling a git fingerprint, only while focused

**Decision.** When the diff shows the working tree or the index, Rain checks a cheap fingerprint
from git: first after `MIN_WAIT`, then twice as long after each check that finds nothing, up to
`MAX_WAIT`, and never sooner than `COST_FACTOR` times what the last check took. Nothing is checked
while the window is not focused. One check runs the moment focus returns.

**Why.** This is the "an agent edits while I review" case, which the watch in entry 10 cannot see.
A fixed fast poll wastes CPU and battery on a slow repository. A check that changes nothing must
redraw nothing.

**Limit.** A change can take up to `MAX_WAIT` to raise the banner. A diff between two commits is
never polled.

**Pinned by:**
- `rain-git::the_edit_mark_follows_what_each_target_shows`
- `rain-app::without_the_focus_nothing_is_checked_and_its_return_checks_once`
- `rain-app::an_idle_working_tree_window_rebuilds_nothing`
- `rain-app::a_slow_check_waits_twenty_times_its_own_cost`
- `rain-app::an_idle_repo_is_checked_twice_a_minute_at_rest`
- `rain-app::a_commit_range_or_a_fixture_is_never_polled`

## 12. Diff text is read-only: one text block per file and fixed-size line records

**Decision.** A file's diff is one string plus a small fixed-size record per line. The view's rows
are small fixed-size values too, and a line's row is found without a per-line map.

**Why.** Rain never edits text, so an editor's rope or piece table would add memory and code for
an operation that never happens. Fixed-size records let a diff of a million lines cost a few bytes
a line beyond its text.

**Limit.** Offsets are 32-bit, so one file's diff text cannot pass 4 GiB.

**Pinned by:**
- `rain-core::line_rec_is_at_most_24_bytes`
- `rain-diffview::a_row_is_sixteen_bytes`
- `rain-diffview::row_of_equals_the_old_hash_map_on_fixture_m`

## 13. One row list for both layouts; split puts both sides in one row

**Decision.** Unified and split draw from the same list of rows. In split, a removed line and the
added line beside it are one row holding both sides. A file with only one side is drawn in a
single column.

**Why.** Two scrolling columns need their scroll positions kept in step and drift apart when
lines wrap. One row cannot drift from itself, and row keys stay the same across a layout switch.

**Limit.** The row type is Rain's own, which is one reason `rain-diffview` is a vendored and
changed copy of the upstream widget; see `crates/rain-diffview/VENDORED.md`.

**Pinned by:**
- `rain-diffview::split_pairs_changes_and_keeps_keys`
- `rain-diffview::one_sided_files_lay_out_single_in_split_with_one_row_per_line`

## 14. Only the first `LINE_CAP` bytes of a line are drawn

**Decision.** A line longer than `LINE_CAP` is drawn, coloured and walked up to the cap. A note
under it says how much is not drawn. Find still reports a match past the cut, and comments still
anchor to the line.

**Why.** A minified line of megabytes made every cursor step cost seconds. The cost of a frame
must follow what is on screen, not what is in the file.

**Limit.** The rest of the line cannot be read in Rain. A comment on a span past the cut has no
highlight to show.

**Pinned by:**
- `rain-diffview::a_very_long_line_costs_what_its_drawn_part_costs`
- `rain-diffview::a_cut_line_says_how_much_is_not_drawn_and_a_normal_line_says_nothing`
- `rain-diffview::the_current_match_past_the_cut_is_named_in_the_note`
- `rain-diffview::comments_anchor_to_a_cut_line_and_a_span_past_the_cut_draws_nothing`
- `crates/rain-diffview/src/state.rs:237` has `pub const LINE_CAP`

## 15. Two highlighters; the slow one never runs on the key-handling thread

**Decision.** Languages with a hand-written linear scanner are coloured at once. Every other
language uses a grammar engine: its lines draw plain first, a worker colours them a chunk at a
time, and only that chunk's rows repaint. The worker's queue is bounded, and a result from before
a file, text or theme change is dropped.

**Why.** Grammar highlighting costs milliseconds a chunk. On the thread that handles keys, that is
a stutter on every scroll into new code. Doing all highlighting ahead of time costs memory for
code nobody scrolls to.

**Limit.** Slow languages show uncoloured text for a moment. The fast scanners are approximate.
Work per source is capped at `MAX_BYTES`.

**Pinned by:**
- `rain-syntax::fast_scanners_take_the_languages_they_know`
- `rain-syntax::only_languages_without_a_fast_scanner_need_the_warm_up`
- `rain-syntax::work_is_capped_at_the_rune_kit_limit`
- `rain-diffview::a_slow_language_is_lexed_off_the_thread_and_comes_out_the_same`
- `rain-diffview::the_worker_keeps_a_bounded_queue_and_dropped_chunks_are_asked_for_again`
- `rain-diffview::a_result_from_before_the_lexer_changed_is_dropped`

## 16. A thread never disappears: it follows its code or becomes an orphan with a reason

**Decision.** A thread stores the code it was posted on. After a reload it moves to where that
code now is, judged by the line and its neighbours. When the code is gone, or two places match
equally well, the thread becomes an orphan that keeps its snapshot and says why.

**Why.** A thread left on its old line number points at the wrong code after an edit above it. A
thread dropped because its line changed loses the review. A guess between two equal matches puts
a comment on code it was never about.

**Limit.** Ambiguous cases need the user to attach the thread by hand.

**Pinned by:**
- `rain-comments::insertion_above_moves_to_the_brace_whose_neighbours_match`
- `rain-comments::equal_candidates_are_ambiguous_orphans_not_guesses`
- `rain-comments::deleted_line_is_orphaned_with_its_post_time_snapshot`
- `rain-comments::rewritten_block_is_orphaned_and_a_gone_file_reads_left_the_diff`

## 17. The agent drives Rain; Rain starts a process only as a fallback, and carefully

**Decision.** An attached agent answers questions through the HTTP routes. Only when none takes a
question does Rain run the user's `ask.command`: an argument list with no shell, in Rain's own
folder and never the repository, the prompt on standard input, a timeout, and the whole process
tree ended with it.
It does not run until the user allows it once.

**Why.** Running an agent from inside the reviewed repository would let files planted in that
repository steer it. A shell would let a file name become a command. The default command sends the
prompt to the user's own account, so it needs a clear yes first.

**Limit.** The fallback knows only what the prompt carries. Each answer is a fresh process.

**Pinned by:**
- `rain-app::an_idle_attached_agent_gets_the_handoff_then_the_launcher_takes_over`
- `rain-app::with_no_agent_attached_the_launcher_answers_at_once`
- `rain-app::the_launcher_waits_until_the_user_allows_it_once`
- `rain-ask::a_missing_program_is_an_error_not_a_shell_fallback`
- `rain-ask::stdout_and_exit_code_come_back_and_stdin_is_the_prompt`
- `rain-ask::a_timeout_ends_the_children_the_command_started_too`

## 18. Two-key shortcuts follow Neovim's rules and cost nothing when unused

**Decision.** A shortcut can be two keys. A first key that only starts shortcuts waits for the
second with no timer. A key that also acts alone waits for the timeout, then acts; another key
that completes nothing makes it act at once and is then read afresh. A key that starts no shortcut
is handled exactly as before.

**Why.** These are the rules Vim users already know. A timer on every held key wakes the window
for nothing, and a lookup on every key would tax plain movement.

**Limit.** A key that both acts alone and starts a shortcut feels slower by the timeout. Rain says
so when such a binding is loaded.

**Pinned by:**
- `rain-app::a_two_key_shortcut_runs_on_its_second_key_and_reads_as_both_keys`
- `rain-app::a_key_that_only_starts_shortcuts_waits_with_no_timer_and_no_view`
- `rain-app::a_key_that_also_acts_alone_waits_the_timeout_then_acts`
- `rain-app::a_key_that_starts_no_shortcut_builds_commits_and_wakes_exactly_as_without_any`
- `rain-app::arrows_and_every_default_key_do_what_they_did_beside_a_two_key_shortcut`

## 19. Speed is held by counts in every test run; clocks are checked by hand

**Decision.** The normal test run holds performance to counts of work: runs of the view, rows
built, layouts, matching passes, allocations and bytes. Each bound is a budget, named beside the
test that holds it. Clock times have budgets too, listed in [perf.md](perf.md), and are checked
alone in a release build.

**Why.** A count repeats exactly. A clock moves by a tenth or more between two runs of the same
code, so a clock check either fails on a busy machine or is set too loose to catch anything. A
count also says which part got dearer.

**Limit.** A count does not see the same work getting slower, such as a colder cache or a slower
allocator. Only the release clock checks and the checks by hand before a release see that, and
they run when someone runs them.

**Pinned by:**
- `rain-app::one_cursor_down_stays_within_its_counts`
- `rain-app::an_idle_frame_rebuilds_nothing`
- `rain-app::a_held_cursor_key_costs_no_more_on_its_tenth_press_than_on_its_first`
- `rain-app::wheel_frames_inside_one_file_lay_nothing_out_and_build_only_the_rows_that_come_into_view`
- `rain-diffview::the_state_of_a_large_diff_stays_within_its_bytes_per_line`
- `rain-diffview::row_keys_are_built_for_the_rows_near_the_viewport_not_for_every_row`
