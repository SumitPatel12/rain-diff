# Counting work in tests

A clock changes by 10% or more from run to run. A count of the work done does not, so speed
tests in `rain-app` check counts. The helper is `crates/rain-app/src/tests/counts.rs`.

## What is counted

| Field | Meaning | Comes from |
|---|---|---|
| `commits` | runs of the whole view | `rune::commit_generation()` |
| `line_rows` | diff line rows built (reused rows are not counted) | `rain_diffview::line_builds()` |
| `fold_rows` | gap and file-note rows built | `rain_diffview::fold_builds()` |
| `layouts` | times a diff laid all its rows out again | `rain_diffview::layout_builds()` |
| `rekeys` | layouts that changed the rows, so every row is keyed again | `rain_diffview::rekeys()` |
| `allocs` | heap allocations | the test binary's counting allocator |
| `matches` | fuzzy passes over the picker's or palette's entries | their caches' `scans` |

All count the calling thread only, so tests side by side don't disturb each other. In the
shipped app a counter is one integer added to; the allocator is in the test binary only.

## Using it

```rust
use super::*; // in a file under crates/rain-app/src/tests/

let mut rt = runtime(app(story(), true));
let c = counts::key_counts(&mut rt, Key::Text("j".into()), Mods::default());
assert!(c.line_rows <= 3 && c.layouts == 0, "{c:?}");
```

- `counts::key_counts(&mut rt, key, mods)`: one key, then frames until the window is at rest.
- `counts::idle_counts(&mut rt, frames)`: that many frames with no input.
- `counts::counted(&mut rt, |rt| { ... })`: anything else, such as several keys or a wait.
- `c.at_most(max, "name")`: prints the counts and fails on the first field over its bound.

Do the action once before you count it, so one-time set-up is not in the number.

## Budgets

The bounds are the constants above the tests in `counts.rs`: what was counted in a debug test
build, with 2% of room on allocations only (that count can move by one). They are budgets, listed
with the clock budgets in [perf.md](perf.md). When a fix lowers a count, lower its constant in
the same commit. Raising one needs the reason in the commit message.
