# CLI and compatibility with the difit tooling

Rain keeps difit's launch contract and comment routes, so tooling written
against difit can switch to Rain with small edits.

## What stays the same

| difit | Rain |
|---|---|
| `difit <target> <base> --merge-base --background` prints one line `{"port","url","pid"}` and exits | same line, same key order (`{"port":4966,"url":"http://localhost:4966","pid":1234}`), exit 0; exit 1 with the child's stderr on failure or after 10 s |
| `--comment <json>` (repeatable), `--include-untracked`, `--clean`, `--context <n>`, `--port <n>`, `--overview <file>`, `--keep-alive`, `--no-open` | same flags (`--keep-alive`/`--no-open` accepted and ignored); `--comment` also takes `@file` |
| port 4966, next free port when taken | same; a Node listener on `::`/`0.0.0.0` also counts as taken |
| `comment add [json] --port P` (arg or stdin) → `{"success","importId","count","warnings"}` | same output keys and order, same validation error texts |
| `comment get --port P --format text\|json` | JSON is byte-identical to what `node …/difit/dist/cli/index.js comment get` prints against the same server (tested) |
| `comment resolve\|remove <ids…> --port P` → `{"success","resolved","notFound","errors"}`, exit 1 on any miss | same |
| `overview set <file> --port P [--self id] [--link id=url]…`, `overview get`, `overview validate` | same; `validate` also prints `{"valid":false,"issues":[…]}` on stdout when it fails |
| routes `POST /api/comment-imports`, `GET /api/comments-json`, `GET /api/comments-output`, `POST /api/comments`, `DELETE /api/comments/:id`, `GET`/`PUT /api/overview` | same paths, bodies and status codes; difit's Node CLI works against Rain unchanged |
| connection error `Cannot connect to difit server on port N` | `Cannot connect to Rain server on port N. Is the server running?` |

Additive: threads may carry `span`, `disposition`, `repo`, `outdated`; `comments-output` may carry `Disposition:` and
`Selection:` lines; `GET /api/health` (`{"ok","app","appVersion","pid","session","version"}`; `app` is `"rain"`, `"redline"`
before the rename, and no client checks it); `?repo=<id>` on the
comment routes scopes reads and imports in a multi-repo window.

Round 2 (all additive; every existing route and readback is byte-compatible, also with notes present — pinned by
the `comments-json` golden test). The full agent contract is [agent.md](agent.md).

| Addition | Shape |
|---|---|
| `GET /api/health` | adds `repos: [ids]`, `threads: n`, `notes: n` |
| `GET /api/notes[?repo=&filePath=]` | `{"notesVersion","notes":[…]}`: stored notes, then the overview's `notes[]` as `overview:<id>` line notes |
| `POST /api/notes[?repo=&replace=1]` | a note or an array → `{success,changed,count,ids,notesVersion}`; upsert by `id` |
| `DELETE /api/notes/:id` | `{success,id,notesVersion}`; 404 unknown, 409 for `overview:` ids |
| `POST /api/goto` | `{path,line?,side?}` or `{threadId}` → the window scrolls there |
| `POST /api/comment-imports` | reply by `{"type":"reply","threadId"}` (warning `No thread with id <id>.`, no fallback); `<repoId>/<path>` paths split on a known repo id when no `repo` is given |
| CLI | `note add\|list\|remove`, `comment reply`, `comment resolve --file --line`, `goto`, `health` |
| `GET /api/events?since=&timeout=` (extras) | long poll: `{"version","changed","threads","notes"}` once `version` passes `since`, or at the timeout (default 25 s, max 60) |
| CLI (extras) | `comment wait --port P [--since V] [--timeout S] [--format json\|text]` (exit 2 on timeout), `comment reply --file --lines A-B` |
| `GET /api/overview` | `coverage` filled from the window's diff (was always empty); adds `coverageByRepo: [coverage…]` and `diffLoaded`; `selfRepo` is null in a multi-repo window unless `--self` names one |
| CLI | `overview render <file> --out <dir> [--size WxH]`: every view to PNGs, dark and light, headless |

Window keys added with extras: `Shift+Y` copies `<repoId>/<path>:<line>` (one-repo windows: `<path>:<line>`), `Ctrl+O`
(and the file header's Open button) opens the file at the cursor line with the `editor` setting (default
`code --goto "{path}:{line}"`; it replaces the old `$EDITOR` lookup).

Notes never appear in `comments-json`, `comments-output`, `comment get` or `POST /api/comments`; they bump the session
`version` but not the comments `version`, so a client's stale check is unaffected.

Round 3 (additive, with one semantic change). Every round-2 route and readback keeps its shape; the `comments-json`
and `comments-output` goldens are unchanged while every thread is open.

**The semantic change.** `DELETE /api/comments/:id` no longer deletes: it sets the thread's `state` to `resolved`
and keeps its history. The reply is the same JSON (`{"success","threadId","version"}`), a missing id is still 404, and
`version` only moves when the state changed. A resolved thread leaves the default readback (`comments-json`,
`comments-output`, the old `events` shape); `?all=1` shows it. `comment resolve|remove` therefore behave as before
from the client's point of view.

**`POST /api/comments` upserts.** difit's route replaces the whole list; Rain's merges by id and keeps every thread
the body leaves out, so a read (open threads only, or one repo) followed by a post can't drop resolved or other-repo
threads. Removing is `DELETE /api/comments/:id`. None of the CLI commands above post to this route (it is difit's browser
editor sync, which Rain doesn't serve). The reply adds
`conflicts: [ids]` when a stale post's state, kind or blocking lost to a newer stored copy.

| Addition | Shape |
|---|---|
| `GET /api/comments-json`, `/api/comments-output` | `?all=1` includes threads whose `state` is not `open`; threads may carry `kind`, `blocking`, `state`, `stateSha`, `stateAt`, `dismissReason`, `anchorState`, `anchorHow`, `orphan`, `target`, `answer` (each omitted at its default) |
| `POST /api/comments/:id/state` | `{state: open\|addressed\|resolved\|dismissed, sha?, reason?, author?}` → `{success,threadId,version,changed}`; `dismissed` needs `reason` (`false-positive\|out-of-scope\|style\|duplicate\|other`); `sha` is kept for `addressed` only; 404 unknown id |
| `GET /api/health` | adds `targetSha: [{repo,base,target}]`, `diffLoaded`, `stale` (HEAD moved since the load), `round` (the round in progress, from 1), `agent: {state,seenAgo}`, `asks: {running,queued}`, `lock: {path}\|null` |
| `GET /api/events?kinds=…` | typed deltas: `{version,seq,changed,gap,events:[{seq,at,kind,author?,thread?,data}],presence}`; `gap` is true when events after `since` fell off the 500-event ring. `kinds` is a comma list of `thread,reply,disposition,state,viewed,ask,answer,submitted,reload,presence,brief,notes,overview` (empty = all); `since=<seq>` (default: the newest now); `author=<name>` leaves that author's events out; `for=agent` marks the caller as the attached agent (heartbeat) and, without `author`, leaves out `claude`'s own events. Without `kinds` the round-2 shape is unchanged |
| `GET/POST /api/presence` | POST `{state: idle\|working\|blocked\|done\|failed, on?, reason?}` → `{success}`; GET `{state,on,reason,seenAt,seenAgo}`; `offline` after 60 s without a heartbeat |
| `GET /api/state` | the window's published reading state (`file,line,side,selection,mode,filter,zoomed`) plus `reader`, `viewed: [{key,hash,hunks}]`, `round`, `place` |
| `POST /api/reload` | `{author?}` → `{success,version}`; the window reloads the diff |
| `POST /api/close` | `{success}`; the window saves and exits 0, as on WM_CLOSE |
| `POST /api/highlight` | `{path, lines:[a,b], side?, caption?, step?:[i,n]}` → `{success}`; the reader spotlights the range |
| `GET/PUT /api/brief` | PUT a `rain-brief/v1` object → `{success,version,stale}` (400 on a non-object or a bad shape); GET `{brief,version,stale,check}` (`check` = `{phantomPaths,citesOutsideDiff,filesNotCited}` once the window installs its checker, `{}` before); `stale` when `writtenAgainst` names another sha than the loaded one |
| `GET /api/triage` | the window's ordered groups and items (`{shape,groups,items,facts}`), or `{diffLoaded:false,…}` before the first load |
| `GET /api/review`, `GET /api/review/:n`, `POST /api/review` | `{round,rounds}`; one round or 404; POST `{verdict: approve\|request-changes\|comment, summary?}` → `{success,round}` (Finish review from the API) |
| `GET /api/lock` and `sessions/<id>.lock` | `{port,pid,startedAt}` written when a window with a session file starts serving, removed when it stops; stale when `GET /api/health` on that port answers with another pid or not at all (a 503 counts as busy, not gone). The file also holds `token`; a stale lock is taken over under `<id>.lock.takeover`, so two windows can't both take it |

Host, Origin, Content-Type and the 50 MB body limit apply to every new route as before.

Round 4 (additive for difit clients; difit's Node CLI still works unchanged):

| Change | Shape |
|---|---|
| handshake | `{"port","url","pid"}` keep difit's order; Rain adds `"token":"<32 hex>"` after them, and `"readOnly":true` when another window holds the session (after `"reused"` on a reused line) |
| read-only window | every route that changes the session answers **423** `{"error":"This window is read-only: another Rain window (pid N, port P) holds session <id>. Send changes to port P."}` instead of `success:true`; reads, `reload`, `close`, `presence`, `goto`, `highlight` still work |
| `X-Rain-Token` | the handshake's `token` (also in `sessions/<id>.lock`, never in `GET /api/lock` or `/api/health`). Optional on every route: without it, writes still apply, but a new author-less `question` or an `answer` made `queued` does not start the window's answerer (the thread waits for the user's Ask). Rain's CLI sends it; difit's never asks questions, so it is unaffected |
| `POST /api/comments/state` | 404 (was a 500 from a panic) |
| `GET /api/health` | answered even when 32 requests are in flight (other routes get 503 then), so a busy window's lock is never taken over |

**Posture.** The server binds loopback only and refuses browsers, so the remaining audience is
local processes. Reads (`comments-json`, `comments-output`, `events`, `triage`, `brief`…) and
difit's write routes stay open to any local process, as difit's are, so difit tooling keeps
working without a token. What the token guards is the one thing a write can trigger beyond the
session: running the answerer (`ask.command`, by default `claude` with read access to the repos).
On a machine shared with other users, another user can still read and write the session through
the port; don't open sensitive reviews there.
