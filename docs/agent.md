# Driving a running Rain (for agents)

Everything here works against an **open window**: each call changes the session, saves it and
updates the window live. The CLI is a thin client of the HTTP routes; use whichever is easier.
Examples are PowerShell; `$r` is the Rain binary
(`$r = '<repo>\target\release\Rain.exe'` or wherever it was built).

## 0. The round-3 loop

```powershell
& $r open --repo C:\src\server HEAD origin/development --merge-base --session review-ACME-1 --background   # 1 open (or reuse)
& $r triage --session review-ACME-1                         # 2 what changed, grouped, in reading order
& $r brief set '@C:\tmp\brief.json' --session review-ACME-1 # 3 the narrative: intent, how, invariants, tour
& $r brief check --session review-ACME-1                    #   fix phantom paths / uncited files, set again
& $r comment add '@C:\tmp\findings.json' --session review-ACME-1   # 4 findings; ids from `comment get --format json`
& $r watch --for agent --kinds reply,ask,state,disposition --session review-ACME-1 --timeout 60   # 5 wait for the user
#   6 an `ask` event = the user asked a question: answer it with `comment reply --thread <id>`
#   7 fix the code, commit, then:
& $r comment set-state k3x9f2 addressed --sha d4e5f6a --session review-ACME-1
& $r reload --session review-ACME-1                         # 8 the window reloads in place; threads re-anchor
& $r review get --session review-ACME-1                     # 9 the user's Finish review: verdict, summary, outcomes
```

Repeat 5–8 until `review get` shows a round with the verdict you need.

## 1. Open and find the window

`rain open [<target> [<base>]] [flags]` (or the same without `open`; a revision literally named
`open` needs the bare form) opens a window over `git diff <base> <target>`:

| Flag | Does |
|---|---|
| `<target>` / `<base>` | a commit-ish, or `.` (all uncommitted), `working` (unstaged), `staged`; base defaults to the target's parent (`HEAD` for `.`/`staged`) |
| `--merge-base` | diff against `git merge-base <target> <base>` |
| `--repo <dir>` | a repository to show; repeat for one window over several repos (default: the current folder) |
| `--session <id>` | session name; threads, viewed state and the reading place persist in `%LOCALAPPDATA%\Rain\sessions\<id>.json`. Default `<repo folder>-<branch or target>`, then `-uncommitted`/`-working`/`-staged` for those targets, then `-<16 hex>` from the absolute repo paths, target, base, merge-base, untracked, whitespace and context options. Fixtures use `fixture-<stem>-<16 hex>` from their absolute path. Pass `--session` to choose the name yourself or reopen an older automatic name |
| `--last` | reopen the most recently saved session as it was opened |
| `--background` | start detached, print the handshake and exit |
| `--port <n>` | preferred port (the next free one is used when taken) |
| `--comment <JSON or @file>` | comment imports to load at start (repeatable) |
| `--overview <file>` | overview document to load at start |
| `--include-untracked` | show untracked files as added |
| `--ignore-whitespace` / `--no-ignore-whitespace` | hide / show whitespace-only changes (shown by default) |
| `--context <n>` | context lines (default 3) |
| `--clean` | start from an empty session |
| `--goto <repo/path:line>` | open at that line |
| `--fixture <file.patch>` | show a patch file instead of running git |
| `--keep-alive`, `--no-open` | accepted for difit compatibility; no effect |

- **Handshake.** `rain … --background` prints one line and exits:
  `{"port":4966,"url":"http://localhost:4966","pid":12345,"token":"<32 hex>"}`. `token` is the
  window's secret (also in its lock file, readable only by you; see §2). `"readOnly":true` is added
  when another window already holds the session (two opens raced): that window serves the session
  but refuses every change with **423** and a message naming the window that holds it, and the CLI
  warns on stderr. Use `--session <id>` (which finds the holder through the lock), not that port.
- **By session.** Every client subcommand takes `--session <id>` instead of `--port <port>`: the
  port comes from the lock file `%LOCALAPPDATA%\Rain\sessions\<id>.lock`
  (`{"port","pid","startedAt"}`, written while the window runs, removed when it closes). Both given:
  `--port` wins. Neither: `RAIN_PORT`, then `RAIN_SESSION` from the environment (the question
  launcher sets both, §6). A missing lock prints `Error: No Rain is open for session <id> (no …)`;
  a lock whose window is gone prints `… is not running (stale lock …)`; exit 1 either way.
- **Reuse.** Opening a session whose window is already running starts nothing: it prints that
  window's handshake with `"reused":true`
  (`{"port":4966,"url":"http://localhost:4966","pid":12345,"reused":true,"token":"…"}`), after handing it the
  call's `--comment` imports (author-less, the reviewer's, as on a fresh open), `--overview` and
  `--goto`. `--clean` is ignored then (with a warning). Two windows never write one session file.
- **Reopen the last one.** `rain open --last [--background]`: the most recently saved session,
  opened with the repos, revisions and diff flags it was last opened with (kept in
  `%LOCALAPPDATA%\Rain\launches\<id>.json`); `--port`, `--goto`, `--comment` given with it still
  apply. A session saved before launch records existed says so and exits 1.
- **Check it.** `rain health` (`GET /api/health`):
  `{"ok":true,"app":"rain","appVersion","pid","session","version","repos":["server"],"threads","notes",
  "targetSha":[{"repo","base","target"}],"diffLoaded":true,"stale":false,"round":1,"agent":{"state":"idle","seenAgo":3},
  "asks":{"running":0,"queued":0},"lock":{"path":"…\\review-ACME-1.lock"}}`.
  `repos` lists the window's repo ids. `GET /api/lock` → `{"path","lock":{"port","pid","startedAt"}}`.

## 2. Rules for every request

- Base URL `http://localhost:<port>`. `POST`/`PUT` need `Content-Type: application/json`
  (`PUT /api/brief` also takes `text/markdown`, §4.1).
- No `Origin` or `Sec-Fetch-*` header (browser requests get **403** `Browser requests are not accepted`), and the
  `Host` must be `localhost`, `127.0.0.1` or `[::1]` with the port (else **403**
  `Unexpected Host header`). A non-JSON `POST`/`PUT` gets **415**
  `Content-Type must be application/json`.
- **Paths.** In a one-repo window a path is repo-relative (`src/x.cs`). In a multi-repo window
  write `<repoId>/<path>` (`server/src/x.cs`); a known repo id prefix is split off. `--repo <id>`
  (CLI) or `?repo=<id>` (HTTP) instead makes the path relative to that repo.
- **Lines** are 1-based numbers of the new file (`side: "new"`, default) or of the old file
  (`side: "old"`, for lines that exist only on the deleted side).
- **`@file`.** Arguments that take `JSON|@FILE` or `TEXT|@FILE` read the file when the value starts
  with `@`. PowerShell treats a bare `@name` as splatting, so quote it: `--body '@C:\tmp\note.md'`.
  Leave the value out to read stdin.
- **Token.** Send the window's token as `X-Rain-Token: <token>` on writes (the CLI finds it in the
  lock file by port or session and sends it itself). Without it every route still works, except
  that a new reviewer question (or an `answer` set to `queued`) does not start the window's
  answerer: the thread is kept and waits for the user's Ask. Reads need no token.
- **Read-only window** (`"readOnly":true` in the handshake): every route that changes the session
  answers **423** `This window is read-only: another Rain window (pid N, port P) holds session <id>…`;
  reads, `reload`, `close`, `presence`, `goto` and `highlight` still work.
- **CLI errors** print `Error: <text>` on stderr and exit 1; connection failures print
  `Error: Cannot connect to Rain server on port <port>. Is the server running?`.

## 3. Triage — `rain triage`

```powershell
& $r triage --session review-ACME-1                 # text: shape, groups, facts, then every file in reading order
& $r triage --session review-ACME-1 --format json   # GET /api/triage
```
`GET /api/triage` → `{"shape":"dense","groups":[{"group":"formatting-only","count":5,"lines":120,
"trusted":true,"reason":"-w empty"}],"items":[{"key","group","reason":{"group","trusted","text"},"risk"?,
"why"?,"realLine"?,"pair"?,"movedFrom"?}],"facts":{"files","lines","real","tests","testsAdded",
"testsRemoved","testsOff":[{"key","line","marker"}],"migrations","renames","binaries","uncited"?}}`.
Groups: `must`, `review`, `skim`, `tests`, `moves`, `formatting-only`, `generated`, `lockfile`,
`huge`, `binary`, `ignored`. `items` is the one order the reviewer walks. Read this instead of the
whole diff: write findings on the real changes, explain the groups in the Brief. Before the window
has published a triage the route answers `{"diffLoaded":false,"groups":[],"items":[]}` and the text
form says the diff is not loaded yet.

## 4. The Brief — `rain brief set`, `rain brief get`, `rain brief check`

```powershell
& $r brief set '@C:\tmp\brief.json' --session review-ACME-1   # PUT /api/brief
& $r brief set C:\tmp\brief.md --session review-ACME-1        # the same Brief written as Markdown (§4.1)
& $r brief get --session review-ACME-1                        # GET /api/brief
& $r brief check --session review-ACME-1                      # text; exit 1 when it reports anything
& $r brief check --session review-ACME-1 --format json
```
The document (`rain-brief/v1`; ids are yours and must stay stable between versions, Rain assigns
`how.N` when one is missing):
```jsonc
{ "format": "rain-brief/v1", "title": "Vouchers at checkout",
  "writtenAgainst": [{"repo":"server","sha":"a1b2c3d"}],
  "intent":     {"id":"intent", "text":"md", "cites":[Cite]},
  "how":        [{"id":"how.1","title":"The API records a voucher","text":"md","cites":[Cite]}],
  "invariants": [{"id":"inv.1","text":"voucher ≤ account total","enforced":[Cite],"tested":[Cite]}],
  "notChanged": {"id":"not-changed","text":"md"},
  "notCovered": {"id":"not-covered","text":"md","paths":["server/src/Migrations/*Snapshot.cs"]},
  "verify":     {"id":"verify","text":"md"},
  "tour":       [{"id":"tour.1","title":"The endpoint","text":"1–3 sentences","cite":Cite}] }
// Cite = {"path":"<repo>/<path>","lines":[a,b]?,"side":"new"|"old"?,"thread":"F1"?}
```
`tour` is optional (default: one step per `how[]` cite). Backticked `File.cs:27-31` in prose links
to the diff. `PUT` → `{"success":true,"version":2,"stale":false,"threadsMoved":0}`; 400
`Invalid brief: …` (not an object, or another `format`). `GET` →
`{"brief","version","stale","check":{"phantomPaths":[],"citesOutsideDiff":[{"section","path","lines"}],
"filesNotCited":[],"stale":[{"repo","written","loaded"}]},"verify":{"items":[{"text","done","source"}],
"removed":[]}}` (`verify`: §4.2). `check` text lines:
`phantom path: <path> (not in the diff)`, `cite outside the diff: <section> cites <path>:<a>-<b>`,
`not cited: <key>`, `stale: <repo> written against <sha>, loaded <sha>`, or
`brief check: nothing to fix`. Generated, formatting-only and `notCovered` files are never
"not cited". A `PUT` re-anchors the threads on Brief sections (§5.6).

Every `text` field is **CommonMark** and is rendered: bold, italics, code, lists, links and fenced
code show as such. Headings inside a body are demoted below the section's own title, so write
`###` freely without breaking the page. Code spans that read as a path (`File.cs`, `a/b.ts:27-31`)
become clickable cites: they open a peek of the cited code and jump to it in the diff.

### 4.1 Writing the Brief as Markdown

Instead of JSON, send one Markdown document: `rain brief set brief.md` (or `'@brief.md'`, or text
starting with `#` on stdin), or `PUT /api/brief` with `Content-Type: text/markdown`. Rain converts it
to the same `rain-brief/v1` document and runs the same check, so `brief get` shows JSON either way.

````markdown
# Vouchers at checkout
## Intent
Adds an optional voucher to account payments. `server/src/Api/Voucher2Controller.cs:20-27` takes it.
## How it works
### The API records a voucher {#how.api}
`POST /voucher` validates the amount. Cites: `server/src/Api/Voucher2Controller.cs:20-27`
## Invariants
- A voucher is never more than the account total. {#inv.cap}
  Enforced: `server/src/Api/Voucher2Controller.cs:23` Tested: `app/src/app/voucher1/voucher1.service.spec.ts`
## Not changed
## Not covered
Generated i18n files.
```paths
app/src/assets/i18n/*.json
```
## Verify by hand
## Tour
1. **The endpoint** `server/src/Api/Voucher2Controller.cs:20-27` Start where a voucher enters the system.
````

| Markdown | Becomes |
|---|---|
| `# Title` (first line, once) | `title` |
| `## Intent`, `## Not changed`, `## Verify by hand` (or `## Verify`) | `intent`, `notChanged`, `verify`: the body as written; an empty section is left out |
| `## How it works` (or `## How`), then `### Step title` per step | `how[]`: `title` = the heading, `text` = its body; only `###` splits, deeper headings stay in the text |
| `## Invariants`, one list item each | `invariants[]`: `text` = the item up to `Enforced:` / `Tested:`; the code spans after each label fill `enforced` / `tested` |
| `## Not covered` | `notCovered`: the text, plus the lines of a ```` ```paths ```` fence as `paths` (globs) |
| `## Tour`, one list item each | `tour[]`: the first `**bold**` = `title`, the first code span = `cite`, the rest = `text` |
| a code span that reads as a path in Intent, How, Not changed or Verify | added to that section's `cites` (once each) and kept in the text |

- **Stable ids.** Put `{#id}` at the end of a heading (`### The API records a voucher {#how.api}`,
  `## Intent {#why}`) or of a list item (`... total. {#inv.cap}`). Without one, ids are assigned as
  for JSON (`intent`, `how.N`, `inv.N`, `not-changed`, `not-covered`, `verify`, `tour.N`), which
  move when you insert a step; give ids to anything a reviewer may already have commented on.
- **A path** is a code span with a `.`, no spaces or brackets, optionally ending `:a` or `:a-b`
  (`server/src/x.cs:20-27`). Code inside fenced blocks never counts.
- **`writtenAgainst`** is filled from the window's loaded shas. A side, a thread link or a cite not
  mentioned in the text need the JSON form.
- **Errors** are a 400 that names the line: ``Invalid brief: line 4: unknown section `## Risks`; use
  one of: Intent, How it works, Invariants, Not changed, Not covered, Verify by hand, Tour``. Section names are
  case-insensitive; each appears once; text goes under a `##` section; How text sits under `###`
  steps; Invariants and Tour are list items; a tour step needs a bold title and a path.

### 4.2 The Verify checklist and the reviewer's edits

Verify's list items (or, without a list, its paragraphs) become a checklist floating over the diff.
The reviewer ticks items, rewords or removes any of them, including yours, and adds their own; all of
it is saved in the session apart from the Brief, so it survives a reload, `rain reload` and a new
`brief set`. After a new Brief the list is: your new items in order, each replaced by the reviewer's
rewording of it (matched by its exact old text) and minus the ones they removed; then rewordings of
items your new Brief no longer has (an edit is never dropped); then the reviewer's own items. An item
you drop that the reviewer never touched goes away; one they removed stays removed if you send it
again. A tick follows its item through a rewording.

Read the list as the reviewer has it from `rain brief get` (`GET /api/brief`) → `verify`:
`items[]` in screen order, each `{"text","done","source"}` with `source` `brief` (yours, untouched),
`edited` (yours, reworded by the reviewer) or `added` (theirs); `removed[]` is the text of your items
they removed. Rain never writes the edits back into the Brief.

## 5. Comments (threads)

Threads are the review conversation; the user reads and answers them in the window. Notes (§7)
never come back through comment readback.

### 5.1 Add — `rain comment add`
```powershell
& $r comment add '[{"type":"thread","filePath":"server/src/VoucherService.cs","position":{"side":"new","line":27},"body":"F1 issue (blocking): amount is not validated.","author":"claude"}]' --session review-ACME-1
& $r comment add '@C:\tmp\findings.json' --session review-ACME-1 --repo server --kind issue
& $r comment add --file server/src/VoucherService.cs --lines 27-31 --kind suggestion --body 'Move RefreshTotal into the unit of work.' --session review-ACME-1
& $r comment add --file server/src/VoucherService.cs --file-level --body '@C:\tmp\why.md' --session review-ACME-1
& $r comment add --pr-level --kind question --body 'Is a migration needed for existing accounts?' --session review-ACME-1
```
- JSON: one import or an array, as text, `@file` or stdin:
  `{"type":"thread","filePath","position":{"side":"new","line":12 | {"start":12,"end":18}},"body",
  "author"?,"kind"?,"blocking"?,"target"?,"state"?,"answer"?}`.
- `--kind comment|issue|question|nit|suggestion|praise` sets the kind of every thread that names
  none. A body starting `F1 issue (blocking):` reads as kind `issue`, blocking, without `--kind`.
- `--file-level` turns every thread into a file-level one (`target.kind = file`, shown under the
  file header); `--pr-level` into one about the whole change (`filePath: "@pr"`, top of the
  Conversation). Position defaults to line 1 for both.
- Flag form (one thread): `--body` with `--file` + `--line`/`--lines` [`--side`], or `--file
  --file-level`, or `--pr-level`.
- `--author` (default `claude`) fills every thread or reply that names none, in both forms. An
  empty author (`--author=`) means the reviewer: a reviewer's `kind: question` starts the window's
  answerer (over HTTP only with the window's token, §2).
- HTTP: `POST /api/comment-imports[?repo=server]` →
  `{"success":true,"changed":true,"count":1,"importId":"<sha256>","warnings":[]}` (the CLI prints
  `{"success","importId","count","warnings"}`). The same file, side, line and body twice is not added
  again. 400 `Invalid comment import data`. Warnings (an import that didn't anchor or apply) print
  as `Error: <text>` after the JSON and exit **3**, for `comment add` and `comment reply` alike.

### 5.2 Comments on the Brief, the Map and notes
Artifact threads use a virtual `filePath` and a `target`:

| On | `filePath` | `target` | Readback location line |
|---|---|---|---|
| Brief section | `@brief` | `{"kind":"brief","section":"how.2","quote":"same unit of work"}` | `Brief › How 2 "One Voucher row in the same unit of work" (quote: "same unit of work")` |
| Map node | `@map` | `{"kind":"node","node":"voucher-service","view":"architecture"}` | `Map › node voucher-service (VoucherService, server/src/Services/VoucherService.cs)` |
| Note | `@note:<noteId>` | `{"kind":"note","note":"k7p2qa"}` | `Note k7p2qa on server/src/Api/VouchersController.cs:L22 "Order matters…"` |
| File | the file | `{"kind":"file"}` | `server/src/Api/VouchersController.cs (file)` |
| Whole change | `@pr` | `{"kind":"pr"}` | `Review (whole change)` |

Position is `{"side":"new","line":1}`. The user writes most of these from the window (`c`/`?` on a
Brief section, a Map node or a note); you read them like any thread and reply by id. difit's own
readers see a thread on a strange path; Rain and agents read `target`.

### 5.3 Read back — `rain comment get`
```powershell
& $r comment get --session review-ACME-1 --format json
& $r comment get --session review-ACME-1 --format text --repo server
& $r comment get --session review-ACME-1 --all          # also resolved, dismissed and addressed threads
```
HTTP: `GET /api/comments-json[?repo=][&all=1]` → `{"version":3,"threads":[…]}` (difit's shape,
byte-compatible for threads without round-3 data), `GET /api/comments-output[?repo=][&all=1]` →
the prompt text. Without `all` only **open** threads are listed. Round-3 fields on a thread (each
only when set): `kind`, `blocking`, `state` (`open|addressed|resolved|dismissed`), `stateSha`,
`stateAt`, `dismissReason`, `anchorState` (`placed|moved|outdated|orphaned`), `anchorHow`
(`exact|fuzzy|renamed|section-id|node-id|label`), `orphan` (`{"reason","lastSeenSha","snapshot","at"}`;
reasons `line-deleted|file-deleted|file-renamed|content-rewritten|section-removed|node-removed|note-removed|ambiguous|left-diff`;
`left-diff`: the file is no longer in the diff, e.g. its change was reverted),
`codeSnapshot` (the code it was written against, never replaced), `target`, `answer`
(`{"status":"queued|answering|answered|failed|timeout|cancelled","by":"attached|launcher","startedAt","endedAt","error","messageId"}`).
In text, such a thread reads (after its location line):
```
[id f1] kind: issue (blocking) · state: open · anchor: orphaned(line deleted)
Last seen at a1b2c3d (src/a.rs:L3):
```
then the fenced snapshot (`>` marks the anchored lines), or `Code:` with the snapshot, and
`Answer: answered by claude (attached)` on questions.

### 5.4 Reply — `rain comment reply`
```powershell
& $r comment reply --session review-ACME-1 --thread k3x9f2 --body 'Fixed in d4e5f6a.'
& $r comment reply --session review-ACME-1 --thread k3x9f2 --body '@C:\tmp\reply.md' --author claude
& $r comment reply --session review-ACME-1 --file server/src/VoucherService.cs --line 27 --body 'Done.'
& $r comment reply --session review-ACME-1 --file server/src/VoucherService.cs --lines 27-31 --side new --body 'Reworked.'
```
By id, or by the exact line (`--line`) or range (`--lines`) a thread hangs on (`--side old` for
deleted lines; `--repo` makes `--file` repo-relative). HTTP: `POST /api/comment-imports` with
`{"type":"reply","threadId":"k3x9f2","body":"…","author":"claude"}`. No match: the reply JSON, then
`Error: No thread with id k3x9f2.` and exit 3. Your reply on a **question** marks its answer
`answered`.

### 5.5 Settle — `rain comment set-state`, `rain comment resolve`
```powershell
& $r comment set-state k3x9f2 addressed --sha d4e5f6a --session review-ACME-1   # you fixed it; the user confirms
& $r comment set-state k3x9f2 dismissed --reason out-of-scope --session review-ACME-1
& $r comment set-state k3x9f2 open --session review-ACME-1
& $r comment resolve k3x9f2 a81bc0 --session review-ACME-1
& $r comment resolve --file server/src/VoucherService.cs --line 27 --side new --session review-ACME-1
```
- `addressed` (with the commit that fixes it) is what an agent sets: the thread waits in the user's
  "Addressed?" list until they confirm (`a`) or reopen it. Leave `resolved` to the user unless
  asked. `dismissed` needs `--reason false-positive|out-of-scope|style|duplicate|other`.
  `--author` defaults to `claude`.
- HTTP: `POST /api/comments/<id>/state` `{"state","sha"?,"reason"?,"author"?}` →
  `{"success":true,"threadId","version","changed"}`; 400 `Dismissing needs a reason`, 404
  `Thread not found: <id>`.
- `comment resolve` (alias `remove`) = state `resolved`. **Changed from difit:**
  `DELETE /api/comments/<id>` no longer deletes; it resolves and keeps the history (same reply
  `{"success":true,"threadId","version"}`, 404 unchanged). The thread leaves the default readback;
  `--all` / `?all=1` still shows it. `--file … --line N` resolves every thread covering that line.
  Prints `{"success":true,"resolved":["k3x9f2"],"notFound":[],"errors":[]}`.

### 5.6 How threads survive new code
On every load and reload each thread runs the re-anchor ladder against the code it was written on
(`codeSnapshot`): same text at the anchor → placed; same text elsewhere (ranked by neighbouring
lines) or through git's line map → `moved(exact)`; the file renamed → `moved(renamed)`; the text
changed under it but its neighbours match → `outdated` (an agent reply after the change, or new code
overlapping it, makes it "addressed?" — a suggestion the user confirms, never automatic); nothing →
`orphaned` with a reason and `orphan.snapshot` = the code as it was, with its sha. Orphans never
disappear: the user re-attaches them (`Attach here`) or closes them. Brief threads follow the
section id, then the quote (`moved(fuzzy)`), else `orphaned(section removed)` with the section text;
Map threads follow the node id, then its label (`moved(label)`); note threads follow the note.

## 6. Waiting and answering — `rain watch`

```powershell
& $r watch --for agent --kinds reply,ask,state,disposition --session review-ACME-1 --timeout 60
& $r watch --kinds ask --since 41 --session review-ACME-1 --format text
& $r watch --session review-ACME-1 --author you          # every kind, leaving out the user's own events
```
- Typed deltas from the window's event ring (500 kept). `--kinds` (comma-separated; default every
  kind): `thread`, `reply`, `disposition`, `state`, `viewed`, `ask`, `answer`, `submitted`,
  `reload`, `presence`, `brief`, `notes`, `overview`.
- `--since <seq>`: the `seq` of the last reply you read; default the newest now, so the next event
  returns. `--timeout S`: default 25, the server caps it at 60.
- `--for agent`: you are the **attached agent**. The wait keeps your presence alive (`idle`, the
  dot in the window), and your own events (`author` = `claude`, or `--author <name>`) are left out,
  so your reply never wakes you.
- Exit **0** with events; exit **2** when the timeout passed with none (JSON still printed,
  `"changed":false`; keep its `seq`); exit 1 on errors.
- HTTP: `GET /api/events?kinds=reply,ask[&since=<seq>][&for=agent][&author=claude][&timeout=]` →
  `{"version","seq","changed","gap","events":[{"seq","at","kind","author"?,"thread"?,"data"?}],"presence"}`.
  `"gap":true` means events after `since` fell off the ring: re-read `comment get` before going on.
  `--format text`: `seq N: K event(s)`, then `#<seq> <kind> <thread> by <author> <data>` per line.

Loop:
```powershell
$seq = (& $r watch --kinds reply --timeout 1 --session review-ACME-1 | ConvertFrom-Json).seq
while ($true) {
  $out = & $r watch --for agent --kinds reply,ask,state,disposition,submitted --since $seq --timeout 60 --session review-ACME-1
  if ($LASTEXITCODE -eq 2) { continue }          # nothing yet: wait again
  if ($LASTEXITCODE -ne 0) { break }             # window closed, or an error
  $e = $out | ConvertFrom-Json
  $seq = $e.seq
  foreach ($ev in $e.events) { <# react: answer asks, fix, reply, set-state #> }
}
```

### 6.1 Questions (Comment vs Question mode)
The reviewer's composer has two modes. **Comment** posts a review comment; nothing is expected of
you until you read it. **Question** (`?`, or `@claude` in a comment, or a reply on a question
thread) posts a thread with `kind: question`, `answer.status: queued` and pushes an `ask` event:
`{"threadId","question","filePath","repo","anchor","target","excerpt","title"}`. The user expects
an answer **now**, inline, as a reply to that thread.

Contract for the attached agent:
1. Hold `rain watch --for agent --kinds ask,…` while you work; that marks you attached.
2. On an `ask`: optionally `rain presence working --on <threadId>` (the card then reads
   `Claude is answering… 12 s`), read what you need (`comment get --format json`, the files), then
   `rain comment reply --thread <threadId> --body '…'` — ≤ 150 words, cite `path:line`, say when
   the answer is not in the code you read. Do not edit files to answer a question.
3. `rain presence idle` when done. Your reply sets `answer.status = answered` and moves the thread
   to the user's "Waiting on you".

The launcher answers when no agent does: if no agent is attached, or the attached one is `working`
on something else or `blocked`, or it has not picked the question up within `ask.handoffSec`
(default 20 s), Rain runs `ask.command` from `settings.json` (default
`claude -p --output-format text --allowedTools Read,Grep,Glob --add-dir {repos}`) with the prompt on
stdin: a fixed preamble, the session title and shas, the Brief's intent and the `how` sections
touching the file, the anchored excerpt (withheld for `ask.redact` paths), the thread so far and
the question. Its stdout becomes a reply by `claude` with `answer.by = "launcher"`; a non-zero exit
or empty output marks it `failed` (stderr head on the card, `Retry`), `ask.timeoutSec` (120)
`timeout`. It runs in Rain's own empty folder (`%LOCALAPPDATA%\Rain\run`), never the repo, so a
program planted in the repo can't be picked up (`{repos}` gives it the repos; `--add-dir={repos}`
inside a longer word repeats it once per repo). It runs with `RAIN_PORT`, `RAIN_SESSION`,
`RAIN_THREAD` set, so `rain comment get` and `rain goto` work from inside it without flags. Nothing leaves the machine unless that command sends
it; the default `claude -p` sends the prompt to the user's Claude account, so the launcher waits
until the user allows it once (the toast's `Allow`, or `Ask: allow` in the palette; stored as
`ask.allowed`). A `claude` command also gets `--disallowedTools "Read(//**/<glob>)"` for each
`ask.redact` glob, so it cannot open those files itself; a glob matches at any depth for both
(`config/prod.json` also withholds `server/config/prod.json`). A second answer to the same question appends as another reply. `watch --for agent --kinds ask`
still delivers asks a launcher took; skip a thread whose `answer.by` is `launcher` and whose status
is not `failed`.

### 6.2 Presence — `rain presence`
```powershell
& $r presence working --on k3x9f2 --session review-ACME-1
& $r presence blocked --reason 'needs a database' --session review-ACME-1
& $r presence done --session review-ACME-1
```
States `idle|working|blocked|done|failed|offline`. HTTP: `POST /api/presence`
`{"state","on"?,"reason"?}` → `{"success":true}`; `GET /api/presence` →
`{"state","on","reason","seenAt","seenAgo"}`. With no heartbeat (a post or a `for=agent` wait) for
60 s you read as `offline`.

### 6.3 The round-2 wait — `rain comment wait`
`rain comment wait [--since <version>] [--timeout S] [--format json|text]` (`GET /api/events` without
`kinds`) is kept: it waits for any session change and prints every open thread and note
(`{"version","changed","threads","notes"}`); exit 0 / 2 as above. `--since` here is the session
`version` (from `health`), not an event `seq`.

## 7. Notes (context for the reviewer, not comments)

Notes explain the change: a **story** note for the whole change (the Brief's fallback card), a
**file** note (under the file header) and **line** notes (inline). They are Markdown, persist in
the session and are **never** part of comment readback. The overview's `notes[]` appear as line
notes with ids `overview:<id>` and author `overview`. A Brief replaces most story notes.
Commands: `rain note add`, `rain note list`, `rain note remove`.

```powershell
& $r note add --session review-ACME-1 --story --title 'Vouchers at checkout' --body '@C:\tmp\story.md'
& $r note add --session review-ACME-1 --file server/src/Services/VoucherService.cs --body 'New service; one `Voucher` row per payment.'
& $r note add --session review-ACME-1 --file server/src/Api/VouchersController.cs --line 22 --body 'Order matters: voucher before RefreshTotal.'
& $r note add --session review-ACME-1 --file server/src/Services/AccountService.cs --lines 44-52 --side old --body 'The old total ignored vouchers.'
& $r note add --session review-ACME-1 --story --replace --body 'Second take on the summary.'
& $r note add --session review-ACME-1 --file server/src/Services/VoucherService.cs --id k7p2qa --body 'Revised wording.'
& $r note add --session review-ACME-1 --json '@C:\tmp\notes.json' --repo server
& $r note list --session review-ACME-1 --format json --file server/src/Services/VoucherService.cs
& $r note list --session review-ACME-1 --story
& $r note remove k7p2qa m1n2o3 --session review-ACME-1
```
- `--author` defaults to `claude`; `--title` optional; `--replace` drops notes of the same kind,
  repo, file and author first; `--id` updates in place; `--json` takes one note or an array.
- HTTP: `POST /api/notes[?repo=][&replace=1]` (body: `{"kind":"story|file|line","filePath"?,
  "position"?,"title"?,"body","author"?}` or an array) →
  `{"changed":true,"count":1,"ids":["k7p2qa"],"notesVersion":5,"success":true}`;
  `GET /api/notes[?repo=&filePath=]` → `{"notesVersion","notes":[…]}`;
  `DELETE /api/notes/<id>` → `{"success":true,"id","notesVersion"}`, 404 `Note not found: <id>`,
  409 for `overview:<id>` notes.
- Line notes carry a snapshot and re-anchor like threads; removing a note orphans the comments on
  it (§5.6).

## 8. Point at things

### 8.1 `rain goto`
```powershell
& $r goto server/src/Api/VouchersController.cs:22 --session review-ACME-1
& $r goto src/legacy.cs:7 --side old --session review-ACME-1
& $r goto --thread k3x9f2 --session review-ACME-1
```
`POST /api/goto` `{"path","line"?,"side"?}` or `{"threadId"}` → `{"success":true}`; the status bar
reads `Claude: look at <place>`. 404 `Thread not found: <id>`; 400 `Invalid goto field: line|side`.

### 8.2 `rain highlight` (walkthroughs)
```powershell
& $r highlight server/src/Api/VouchersController.cs:20-27 --caption 'The endpoint' --step 1/4 --session review-ACME-1
& $r highlight src/legacy.cs:7 --side old --session review-ACME-1
```
The reader spotlights the range (the rest dims), scrolls it to the upper third and the status bar
reads `Claude: look at <path>:20-27 · step 1/4 · The endpoint`; moving the cursor ends it.
`POST /api/highlight` `{"path","lines":[a,b],"side"?,"caption"?,"step"?:[i,n]}` → `{"success":true}`;
400 `Invalid highlight field: path|lines|side|step`.

### 8.3 `rain state`
`GET /api/state` → `{"file","line","side","selection"?,"mode","filter","zoomed","reader",
"viewed":[{"key","hash","hunks":[{"id","hash"}]}],"round","place"}` — where the reviewer is and
what they have viewed (hunk-level).

## 9. Lifecycle

- `rain reload` (`POST /api/reload`) → `{"success":true,"version"}`: the window re-runs git and
  triage, keeps the reader on the same text, re-anchors threads and shows
  `Reloaded · 3 moved · 1 addressed? · 2 orphaned`. Call it after you commit a fix. The window also
  watches each repo's HEAD and offers the reload itself.
- `rain close [--timeout 15]` (`POST /api/close`): the window saves and closes; the CLI waits until
  its process is gone (posting it a `WM_CLOSE`, the close button's path, if it is still up after
  2 s) and prints `{"success":true,"pid","port"}`; the lock file goes with it. Exit 1 if it is
  still running after the timeout.
- **Rounds** — `rain review get [--round N] [--format json|text]`: `GET /api/review` →
  `{"round":2,"rounds":[{"n":1,"at","verdict":"approve|request-changes|comment","summary",
  "shas":[{"id","base","target"}],"viewed":[…],"dispositions":[{"thread","disposition"?,"state",
  "dismissReason"?}],"questionsOpen":[],"draftsUnsent":0,"accepted":[{"group","files","reason","at","undone"}]}]}`;
  `GET /api/review/<n>` one round (404 `No review round <n>`). The user finishes a round with
  `Finish review…` (`Shift+F`); a `submitted` event wakes `watch`. `POST /api/review`
  `{"verdict","summary"?,"target"?}` records one from the API (409 when `target` is no longer the
  loaded sha).
- **Log.** A `--background` window logs to `%LOCALAPPDATA%\Rain\logs\<session>-<pid>.log`: when it
  started and why it closed; no closing line means the process was killed.

## 10. Overview and the rest

- `rain overview set <file> --session <id> [--self <repoId>] [--link <repoId>=<url>]` →
  `PUT /api/overview`; `rain overview get` → `GET /api/overview`; `rain overview validate <file>`
  checks a document offline; `rain overview render <file> --out <dir> [--size 1600x1000]` draws
  every view to `<dir>\NN-<view title>-<theme>.png` without a window.
- The overview feeds the Map overlay (`m`; Clean style) and upgrades triage risk and `why` for the
  files it rates.
- **Animated diagrams.** An optional top-level `diagrams` object carries up to four documents, each
  in a rain-lab schema (fields: `crates/rain-lab/schema/README.md`): `tour` (schema A) for the
  request / data flow through the layers, `flowchart` (S) for a step-by-step scene of the change,
  `lineage` (B) for schema or data-model changes (PK/FK, a why-note per card), `icicle` (C) for
  performance-sensitive code or a query before/after. The Map shows them first, as animated tabs
  (Space plays, ←/→ step, Home restarts), the first one selected, then the graph's views. Keep each
  compact (tour ≤ 14 entities and ≤ 8 steps, flowchart ≤ 24 shapes and ≤ 12 beats, lineage ≤ 30
  fields, icicle ≤ 40 frames). `overview validate` checks each against its schema and its ids, with
  paths like `diagrams.tour.paths[0].steps[1].from` (`INVALID_DIAGRAM`, `BROKEN_REFERENCE`,
  `DUPLICATE_ID`); an overview without `diagrams` loads as before. Examples:
  `skills/rain/references/overview-guide.md` §7a.
- On the Map's graph views a click selects a node (its pane: files, threads, Comment / Ask); a
  second click opens its first hunk; ✕ or Esc closes the pane until a node is clicked again. Repo ids: the overview `repos[].id` whose `dir` equals the checkout folder name
  (case-insensitive), else the first `dir` the folder name contains, else the folder name.
- `overview get` returns `coverageByRepo` (`review`, `missingReview`, `ignored`, `generated`,
  `uncovered`, `formattingOnly` per repo) and `coverage` for `selfRepo`; `diffLoaded` is false until
  the diff loads.
- Node kinds: `service`, `app`, `module`, `function`, `route`, `job`, `queue`, `datastore`, `cache`, `external`, `ui`, `config`, `test`, `package`, `other`.
- Edge kinds: `call`, `http`, `rpc`, `event`, `queue`, `data`, `dependency`, `render`, `other`.
- `POST /api/comments` (`{"threads":[…],"baseVersion"}`) upserts threads by id and never deletes
  one it wasn't sent (readback leaves out resolved threads and, with `?repo=`, other repos'); use
  `DELETE /api/comments/:id` to remove. With a current `baseVersion` the posted state, kind and
  blocking win; with a stale one they win only when the thread's `updatedAt` is newer, and the ids
  whose stored values were kept come back in `conflicts`. difit's editor route; agents should
  prefer the imports above. A body without `threads` is refused (400).
- `rain version` prints the version.

Versions (counters that only go up): `version` in `health` and the round-2 `events` = the session;
`seq` in typed events and `watch` = the event ring; `version` in the comment routes = thread changes
only (difit's `baseVersion`); `notesVersion` = notes; `version` in `/api/overview` and
`/api/brief` = that document.

## 11. Suggested agent workflow

- Open with `--session <story id>`: a second open reuses the window (`"reused":true`), and every later
  call uses `--session` instead of tracking ports. `rain open --last` resumes.
- After opening: `rain triage` (read groups instead of the whole diff), then `rain brief set` with
  intent / how / invariants / not covered / verify and an optional `tour`, then `rain brief check`
  until it prints `nothing to fix`. The story note becomes the Brief.
- Findings: `--kind issue|suggestion|nit|question|praise` (or the `F1 issue (blocking):` prefix);
  `--file-level` and `--pr-level` for findings that are not about a line.
- Replace the `comment wait` loop with `rain watch --for agent --kinds reply,ask,state,disposition,submitted`
  held in the background for the whole review; answer every `ask` with `comment reply --thread`
  at once (§6.1) and set presence while working.
- After a fix: commit, `comment set-state <id> addressed --sha <sha>`, `rain reload`. Never resolve
  the user's threads yourself; `comment resolve` no longer deletes.
- `comment get` shows open threads only; use `--all` for history. Read `target` for comments on the
  Brief / Map / notes, and `orphan.snapshot` for threads whose code is gone.
- End: `rain review get` for the verdict, then `rain close`.
- `rain highlight … --step i/n` walks the reviewer through a change; `rain goto` points at one line.
