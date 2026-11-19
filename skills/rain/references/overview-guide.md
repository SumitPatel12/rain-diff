# Overview guide — writing a `difit-overview/v1` document

An overview is one JSON file per story that gives Rain its **Map**: an architecture picture of the
change, a risk-rated reading order for every repo, optional phases, a walkthrough, and intent notes on
the diff. One file serves the story's Rain window, which shows every repo at once. The format name
`difit-overview/v1` is historical; Rain reads it unchanged.

Read by the `rain` skill and any flow that hands a review to Rain.

*Adapted from PR Lens (`packages/cli/src/prompt.ts`, `packages/agent-skill/manual.md`,
`references/graph-document.md`; MIT, Copyright (c) 2026 Coldtea AI) and from the reading-order and
phase conventions of diffmap's `guide.md` (MIT, Copyright (c) 2026 Tanishq Kancharla).*

The examples below use a made-up product with three repos — `api` (`acme-api`, a backend service),
`web` (`acme-web`, a browser app) and `mobile` (`acme-mobile`, a phone app). Swap in your own.

---

## 1. When to write one

Write one when **any** of these holds. Count per repo, then sum:

```
git -C <repo> diff --numstat <merge-base>...<head>
```

- files = rows **not** matched by the ignore seed (§5); lines = Σ(added + deleted) over those rows;
- **≥ 25 changed files**, or
- **≥ 800 changed lines**, or
- **≥ 2 repos with an API-contract change** — a DTO / request model / endpoint in the backend plus the
  generated client model or the caller in a frontend repo.

Below every threshold: skip it unless the user asks. Never run a path-less `git diff` on a repo whose
numstat shows a huge generated file (a 40k-line migration designer file); read hunks per path.

## 2. The wrapper

```jsonc
{
  "format": "difit-overview/v1",            // literal
  "title": "PROJ-0000 Short story title",    // 1-200 chars
  "repos": [                                  // 1-8, ids unique
    {
      "id": "api",                            // ^[a-z][a-z0-9-]{0,31}$ — the prefix of every graph path
      "dir": "acme-api",                      // folder leaf of the clone, matched case-insensitively
      "base": "<sha>",                        // git merge-base origin/main <head>
      "head": "<sha>"                         // git rev-parse <head>, or "WORKTREE" for uncommitted work
    }
  ],
  "graph": { /* a PR Lens graph document, §4 */ },
  "readingOrder": [                           // optional, one entry per repo at most
    { "repo": "api", "review": [{ "path": "src/Api/X.cs", "why": "entry point", "risk": "critical" }],
      "ignore": [{ "glob": "src/Migrations/*.Designer.cs", "reason": "generated" }] }
  ],
  "phases": [                                 // optional, §6
    { "id": "backend", "title": "…", "status": "done", "today": "…", "proposed": "…", "nodeIds": ["…"] }
  ],
  "notes": [                                  // optional, §5b, ≤ 128 (guide budget 8 per repo)
    { "id": "record-before-refresh", "path": "api/src/Api/X.cs", "startLine": 22, "text": "…" }
  ],
  "diagrams": {                               // optional, §7a: the Map's animated diagrams
    "tour": { /* schema A */ }, "flowchart": { /* schema S */ },
    "lineage": { /* schema B */ }, "icicle": { /* schema C */ }
  }
}
```

`risk` (§5a) and `notes` (§5b) are round-2 fields; Rain supports both.

**Path rules** — the one place documents go wrong that PR Lens never had:

| Field | Form | Right | Wrong |
|---|---|---|---|
| graph `files[].path`, `payload.*.source.path`, `dataModel.tables[].files[].path`, `dataModel.tables[].indexes[].files[].path`, file cites, `notes[].path` | `<repoId>/<repo-relative POSIX>` | `api/src/Api/TipsController.cs` | `src/Api/TipsController.cs` (no prefix) · `acme-api/src/…` (dir, not id) |
| `readingOrder[].review[].path` | repo-relative, **no** prefix | `src/Api/TipsController.cs` | `api/src/Api/TipsController.cs` |
| `readingOrder[].ignore[].glob` | repo-relative glob; `**` crosses folders, `*` and `?` do not | `src/Migrations/*.Designer.cs` | `**\Migrations\*.cs` (backslash) |

No leading `/`, no drive letter, no backslash, no `..` anywhere.

**Shas.** `repos[].base` = `git -C <repo> merge-base origin/main <head>` (use your base branch);
`repos[].head` = `git -C <repo> rev-parse <head>`, or `WORKTREE` when the repo's change is uncommitted
(Rain warns, does not refuse). When your branches carry the story key, use the branch that does
(`origin/feature/PROJ-123-<slug>`, not `origin/feature/<slug>`). `graph.provenance` takes the **real**
shas of the first repo — never `WORKTREE` (use `git rev-parse HEAD`) — with `repo.owner: "<your org>"`
(`"acme"` in the examples) and `repo.name: "<first repo dir>"`.

## 3. Ids

`^[A-Za-z0-9][A-Za-z0-9._:/-]*$`, unique in their collection, readable kebab-case (`tip-service`, not
`n1`). Repo ids are lowercase and short: `api`, `web`, `mobile`.

## 4. Writing the graph

The graph is a PR Lens document, contract `schemaVersion: "0.3.0"`, `kind: "graph"`. Every schema is
strict: an unknown key is a rejection.

- **Describe the change only.** The graph has no field for bugs, style or security notes. Findings go
  in comment threads. Attention goes in `readingOrder[].review[].risk` (§5a). Intent goes in `notes` (§5b).
- **Include the unchanged neighbours** the change touches, `delta: "unchanged"`. They make the blast
  radius legible; a diagram of only changed nodes says nothing about impact.
- **Lanes are the reader's mental model, 3-4 of them**, not the folder tree. Example lanes:
  `web` (browser app), `mobile` (phone app), `api` (backend service), `jobs` (background jobs /
  functions), `data` (database), `external` (payment provider, SMS gateway, …). Past four, merge two and
  use `group` on the nodes for the finer split — it costs no width.
- **1-2 hero edges** (`emphasis: "hero"`): the connection the change is really about. Both ends must be
  nodes of the top (`defaultOpen`) view, or the top view never animates.
- **Labels ≤ 20 chars.** A card that shares a row truncates anything longer. Name the role
  (`Receive items API`) and put the class name in `subtitle` (`ReceiveItemsController`).
- **Every node gets `files`** with `startLine` on a **changed line** of its entry point — the controller
  action, the service method, the component class, the client service method — so a click lands on the
  diff, not on context. `endLine` when the hunk spans lines. `revision: "base"` only for removed code
  (it points at the left side of the diff).
- **`dataModel`** only when entities or migrations change: the tables touched with every column and a
  delta each, `previousType` for a changed type, one relation per FK, the unchanged tables a key points
  at; declare the `data-model` lens. Never read a generated migration designer or model snapshot to
  build it — the migration and the entity class are enough.
  - **`files` on every changed table:** the entity class line and the migration line that creates or
    alters it, so a click on the table lands on the diff. Unchanged neighbours may have none.
  - **`indexes`** (round 2): each index the migration creates or drops, plus each index declared in a
    diffed model configuration. Add an unchanged index only when it shows in a diffed file. Each entry
    has `name` (unique within the table), `columns` (in key order, names as in `columns[]`), `unique`,
    `delta` and `files` (the migration or model line). List only the columns that matter to the change.
    An index column missing from `columns[]` draws muted and gives a warning, not an error.

  ```json
  "indexes": [
    { "name": "UX_Tips_InvoiceId_Created", "columns": ["InvoiceId", "CreatedAt"], "unique": true, "delta": "added",
      "files": [{ "path": "api/src/Migrations/20260101000000_AddTip.cs", "startLine": 32, "endLine": 34 }] }
  ]
  ```
- **Flows** only for a sequence worth animating. One good flow beats three thin ones. **≤ 5
  participants** — at 6-7 the flow is 2,300-2,600 px wide and its labels are unreadable at panel width;
  split a longer sequence into two flows. A document with flows declares `data-flow`. Message
  endpoints must be participants of that flow. `self` needs `from === to`.
- **Views (C4 rules).** Start at the highest useful level: context when a user, external system or
  boundary is affected, else containers; add a component child only when a container's internals
  matter. Each child moves down one level and is materially narrower; never two views with
  substantially the same nodes. Cap a view at ~7 nodes and collapse the rest. Never two cards per row
  in a narrow view: a child view of 4-5 nodes over 2 lanes puts two per row and truncates every title
  — leave those nodes to a wider view instead. `defaultOpen: true` on the top architecture view only.
  Data-flow views are separate roots, one per flow, with
  `scope: { "kind": "selection", "flows": ["<flow-id>"] }`. (Rain also makes a picture for every flow
  on its own, so a walkthrough can stage a flow you gave no view.)
- **Stat chips** carry the numbers that matter (`"Endpoints"` / `"+2"`, `"Tables"` / `"+1"`), not file
  or line counts.
- `title` a reviewer would recognise; `summary` answers "what does this change do?" in one paragraph.

### 4a. Keep every picture clean

Aim for as few crossing or overlapping arrows as possible and nothing crowded. A view that is valid but
tangled is not done. The renderer owns the layout, and there are no coordinates to set, so use these
levers (from the renderer's seating rules):

- **Fewer edges per view.** Two sources that both call the same two targets (2×2) always tangle in one
  lane. Split that into two child views, one per target, and keep the top view to the main path.
- **Pair or stack.** Two cards share a row only when they have the same rank and the same `group`. To stack
  a card under its neighbour, give it its own `group`; to pair them, give both the same `group` (or none).
  Stacking makes the arrows straight when each card feeds a different card in the next lane.
  `group` applies in every view, so re-check each view the node is in.
- **Document order breaks ties.** When ranks and partners are equal, nodes keep their order in `nodes[]`,
  so moving a node next to the one it should pair with changes what pairs.
- **Move a card down.** `layout.rank` (`{ "<node id>": n }`) makes `n` the lowest rank a card can have,
  which moves it below other cards. It can never lift a card above one that feeds it. Unused ranks are
  dropped, so it can't leave an empty row. The router already widens a corridor or band when too many
  arrows run through it, so you don't need to add space yourself.
- **Smaller cards.** Card width is fixed: a full lane, or half of it when paired. A long subtitle gets cut
  off, so shorten it (`POST item-codes/preview`, not the class name) or leave it off. A card with no
  subtitle is also shorter.

Then render every view (§8) and look at each picture. Fix any arrow that crosses another or runs under a
label, and any card whose text is cut off.

## 5. Reading order

Every changed file of every repo sits in **exactly one** of:
- `review` — ordered, entry point first, then down the layers; each with a one-line `why`;
- `ignore` — globs with a reason, for files that need no thought.

Rain computes the rest itself and shows **uncovered** files with a warning, so a missing file is
visible, not silent.

**Ignore seed.** Rain already flags these as generated (you need not list them, but listing them is
harmless): lockfiles (`package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `packages.lock.json`, …),
`*.Designer.cs`, `*.g.cs`, `*.min.js`, `*.map`, `*.generated.*`, any path containing
`/generated/`, `/gen/` or `/__generated__/`, and paths under `dist/`, `vendor/`, `node_modules/`.
The match is by path (and `.gitattributes`) only: a file whose header alone says `<auto-generated>` /
`DO NOT EDIT` is **not** flagged, so list it under `ignore` or it shows as uncovered.

Add your project's extras every time they appear in the diff. Examples of common generated files —
add your own:

| Repo | Glob | Reason |
|---|---|---|
| any | `**/.codegen-cache.json` | codegen cache |
| api | `**/Migrations/*ModelSnapshot.cs` | EF Core model snapshot |
| api | `**/Migrations/*.Designer.cs` | EF Core migration designer |
| web | `src/api-client/**` | OpenAPI-generated client |
| mobile | `src/generated/**` | OpenAPI-generated models |
| mobile | `package.json`, `package-lock.json` | version bump (only when the diff is just the version) |

Keep the two `Migrations` rows even though Rain may flag them itself: they work as written, and they
keep a 40k-line designer out of the §1 line count.
Generated rows in a story diff are often unrelated drift from the base branch (models of other
features); ignore them all the same.

### 5a. Risk (round 2)

Rate every `review` entry with `risk`. Rain shows it as a glyph, a colour and a label in the reading
order and the file tree. Take the first step that matches:

1. Formatting or mechanical only (whitespace, rename, reorder, a moved import) → `low`, whatever the
   domain.
2. A logic or behaviour change in a **critical domain** → `critical`. `why` names the domain
   (`payments: tip added to the charged total`).
3. Any other logic change → `high`.
4. Additive wiring or a simple change (a new DTO field, a DI registration, a route entry, a template
   binding) → `medium`.

Generated or vendored files go in `ignore`, not `review`. A hand-edited, formatting-only file stays in
`review` as `low`.

| level | label | when |
|---|---|---|
| `low` | Formatting only | Formatting or mechanical change. No need to look. |
| `medium` | Skim | Simple change. Maybe look it over. |
| `high` | Review | Logic change. Do take a look. |
| `critical` | Must review | Significant change in payments, billing, invoices, account creation, auth, permissions, data deletion, scheduling or sync. Breaking this stops daily work. |

**Critical domains.** A starting list — tailor it to your product, and replace the example paths with
your own. Rate by what the code does, not by the folder it sits in:

- **Payments** — `api/src/Payments/`, `web/src/app/checkout/`, `mobile/src/app/pay/`
- **Billing and invoices** — `api/src/Billing/`, `api/src/Invoices/`, `web/src/app/invoices/`
- **Account creation** — `api/src/Accounts/Signup/`, `web/src/app/onboarding/`
- **Auth and permissions** — `api/src/Auth/`, `api/src/Permissions/`, `web/src/app/core/auth/`, `mobile/src/app/core/auth.interceptor.ts`
- **Data deletion** — `api/src/Accounts/DeleteAccount/`, `api/src/Customers/BulkDelete/`
- **Scheduling** — `api/src/Scheduling/`, `web/src/app/calendar/`
- **Sync** — `api/src/Sync/`, `mobile/src/app/services/offline-sync.service.ts`

**Tie-breaks:**
- A whitespace-only reformat of `PaymentsController.cs` → `low` (step 1 wins over the domain).
- A new optional field on a payment DTO, only passed through → `medium`. When the field changes the
  charged amount → `critical`.
- A new authorization attribute or policy on an existing endpoint → `critical` (auth behaviour changed).
- A calendar CSS or label change → `medium`. A change to who a booking is assigned to → `critical`.
- A new read-only report query outside every critical domain → `high`.
- A spec or test file → one level below the code it covers, so the spec of a `critical` file is `high`.

**Self-check.** More than 30 % of a repo's review files `critical` → re-rate. Critical means "breaking
this stops daily work", not "important". `why` is always shown, so a wrong rating is visible to the
reader. Rain warns on an unrated file and on a `low` or `medium` file whose path looks
domain-critical. Read the warning, keep the rating if step 1 or 4 applies, and fix it if not.

### 5b. Intent notes (round 2)

`notes` are short read-only cards anchored to a diff line. Each answers "why is this code the way it is?"
Rain shows them inline in the diff as "Intent" notes (author `overview`), and the user can pop one out
into its own window. They live in the overview, so `rain-story.ps1 -Mode get` and comment readback never
see them.

Write a note only for:
- an **ordering** constraint (A must run before B);
- a **workaround** (for a library bug, a platform quirk or a data shape);
- an **invariant** the code relies on but does not state;
- a **compatibility** shim (old clients, old rows, a feature flag);
- the **road not taken**, when the obvious approach was rejected.

Rules:
- **Grounded.** Base it on a commit message, the PR description, the story's acceptance criteria or a
  code comment. Without one, start it with "Likely …" or skip it. When reviewing someone else's code,
  this rule is strict.
- **Never a finding.** A problem goes in a comment thread. Never restate what the code does.
- **≤ 8 per repo** (Rain warns past that), `text` 1-600 chars with light markdown. The **first sentence is
  the summary** shown in the list, so make it stand on its own.
- **Anchor** `startLine` (and `endLine`) to a **changed** line on the head side. Use `revision: "base"`
  only for removed code. A line outside every hunk still shows, at the top of the file's first hunk with
  "line not in diff". A note on a file not in the diff shows only in the Comments tab.
- `id` is a readable kebab-case id, unique within `notes`. `path` takes the `<repoId>/` prefix, like graph
  paths.

```json
{ "id": "record-before-refresh", "path": "api/src/Api/TipsController.cs", "startLine": 22,
  "text": "RecordTip runs before the invoice total refresh so the refreshed total already includes the new tip. Swapping the two calls would show the old total until the next reload." }
```

## 6. Phases

Only when the story has **≥ 2 sub-tasks** or **≥ 2 repos that land independently**. Each phase:
`today` and `proposed` written from the user's side (what they see or call), `nodeIds` = the graph
nodes that phase touches, `status` `done` or `pending`. Ids unique; every node id must exist.

## 7. Walkthrough

3-6 steps (the schema allows 2-12). Each step is **one change**, in the order a reviewer needs it; the
headline change is step one; an overview of everything touched, if any, is last.

- `heading` ≤ 48 chars, sentence case, built from change words: added, removed, replaced, now, moved,
  split. If it could have been true before the change, rewrite it.
- `body` ≤ 140 chars, on behaviour: what happens now that did not, or what stops — with numbers.
- `stage` a view (`{ "kind": "view", "view": "<id>" }`) or a flow (`{ "kind": "flow", "flow": "<id>" }`).
  Keep consecutive steps on the same stage together.
- `focus` 2-3 elements (`nodes`, `edges`, `lanes`, `tables`, or `messages` — only when the stage is a
  flow, and only that flow's messages).
- `detail` (optional, ≤ 240 chars): a sentence whose `cites` turn words into links. Each cite's `text`
  is copied **letter for letter** from the detail, cites listed in sentence order, no two with the same
  words, ≤ 8. Refs: `node`, `message` (`flow` + `message`), `view`, `flow`, `table`, `column`, and —
  **unlike the hosted canvas — `file`** (`{ "kind": "file", "path": "api/src/Api/X.cs" }`, prefixed):
  Rain turns a file cite into a jump to the diff. Leave detail off the overview step.

Write every word for a smart twelve-year-old: short common words, one idea per line, active voice,
things named as the diagram names them, numbers as digits. Never "leverages", "orchestrates",
"asynchronous pipeline", "fan-out".

| Write this | Not this |
|---|---|
| Route now queues the job instead of sending / The API call finishes at once. A worker sends the mail later. | Broadcast fan-out moves behind the queue / The API route now enqueues broadcast jobs for asynchronous batch processing instead of sending emails inline. |
| The mail provider now gets 500 emails per call / One call per batch instead of one call per person. | Batched delivery replaces single sends / The worker leverages the shared library to send emails in chunks of 500 via the provider's batch endpoint. |
| processBroadcast and sendSingleEmail removed / sendBroadcastBulk does their job for whole batches. | Single send functions are retired / sendBroadcastBulk replaces processBroadcast and sendSingleEmail to handle bulk deliveries in chunks. |

## 7a. Animated diagrams (`diagrams`)

Optional. Each key holds one document the Map plays as an animation, ahead of the graph's views (the
first one present opens selected; the views stay as tabs after them). Fields, types and defaults:
`crates/rain-lab/schema/README.md` (JSON Schemas beside it). Schemas are strict: unknown keys fail.
Pick only the ones the change needs:

| Key | Schema | Include when | Shows |
|---|---|---|---|
| `tour` | A | almost always: the request / data flow through the layers | components as stations in frontend, backend and database bands, each path a coloured line, tour steps that walk it |
| `flowchart` | S | the change is a sequence worth replaying step by step | your own scene: boxes, arrows and dots brought in beat by beat, a sentence per beat |
| `lineage` | B | a schema or data-model change | columns → entity → DTO → screen, PK/FK tagged, a strand per value, a why-note per card |
| `icicle` | C | performance-sensitive code, or a query before/after | call trees, bars as wide as their self time, red where it grew |

**Size budget**: tour ≤ 14 entities, ≤ 3 paths, ≤ 8 tour steps; flowchart ≤ 24 shapes, ≤ 12
beats, scene about 900 × 500; lineage ≤ 30 fields over ≤ 6 cards; icicle ≤ 40 frames, depth ≤ 5.
Labels ≤ 24 characters, tour and beat text one or two sentences. `validate` warns past the entity,
tour-step, shape, beat, field and frame counts.

Rules that `validate` enforces beyond the schema: ids unique (entities, paths, shapes, fields, frames
across every tree); every `from`/`to`/`carries`/`focus` names an entity, every action `target` a shape
(only `focus` may be empty), every lineage `from`/`to`/`via` and `references` a field; a path has at
least one step; an icicle has `calls` or a profile. Ids of different diagrams are separate.

**tour** — `layer` is `web`/`ui`, `api`/`domain` or `data`/`db`; `[[id]]` in text links an entity or path.

```json
{ "title": "Tips at checkout",
  "entities": [
    { "id": "tip-form", "label": "TipForm", "layer": "web" },
    { "id": "tips-api", "label": "TipsController", "layer": "api" },
    { "id": "tip-service", "label": "TipService", "layer": "domain" },
    { "id": "tips-table", "label": "Tips", "layer": "db" },
    { "id": "tip-dto", "label": "TipDto", "layer": "api", "fields": ["amount"] } ],
  "paths": [ { "id": "add-tip", "name": "Add a tip", "steps": [
    { "from": "tip-form", "to": "tips-api", "call": "POST /tips", "carries": ["tip-dto"], "status": "added" },
    { "from": "tips-api", "to": "tip-service", "call": "Add(dto)", "status": "added" },
    { "from": "tip-service", "to": "tips-table", "call": "INSERT Tips", "status": "added" } ] } ],
  "tour": [
    { "title": "The form posts", "text": "[[tip-form]] posts the amount to [[tips-api]].", "focus": ["tip-form", "tips-api"] },
    { "title": "Stored once", "text": "[[tip-service]] writes one row to [[tips-table]].", "focus": ["tip-service", "tips-table"] } ] }
```

**flowchart** — `size` is the scene in points; `at` is a box's top-left, an arrow's start; beats run
in order of `t` (seconds). Verbs: `create`, `write`, `transform`, `move`, `fade`, `focus`, `pulse`, `morph`.

```json
{ "title": "A tip's journey", "size": [640, 220],
  "shapes": [
    { "id": "form", "kind": "box", "at": [40, 80], "props": { "w": 160, "h": 50, "text": "TipForm", "tone": "added" } },
    { "id": "api", "kind": "box", "at": [440, 80], "props": { "w": 160, "h": 50, "text": "TipsController", "tone": "added" } },
    { "id": "post", "kind": "arrow", "at": [200, 105], "props": { "points": [[200, 105], [440, 105]], "text": "POST /tips", "tone": "added" } } ],
  "beats": [
    { "t": 0, "actions": [{ "do": "create", "target": "form" }], "say": "Staff enter a tip on the new [[form]]." },
    { "t": 1.2, "actions": [{ "do": "write", "target": "post" }, { "do": "create", "target": "api" }], "say": "It posts to [[api]]." },
    { "t": 2.4, "actions": [{ "do": "pulse", "target": "post", "dur": 1 }], "say": "The amount travels with the call." } ] }
```

**lineage** — field ids are `stage:Owner.name` with stage `db`, `entity`, `dto` or `ui`; a missing
`before` means added, a missing `after` removed. Mark keys with `primaryKey` and `references`, and give
each card (`stage:Owner`) a `notes[].why`.

```json
{ "title": "Tips at checkout",
  "fields": [
    { "id": "db:Tips.Id", "stage": "db", "after": { "name": "Id", "type": "int" }, "primaryKey": true },
    { "id": "db:Tips.OrderId", "stage": "db", "after": { "name": "OrderId", "type": "int" }, "references": "db:Orders.Id", "why": "Each tip belongs to one order." },
    { "id": "db:Tips.Amount", "stage": "db", "after": { "name": "Amount", "type": "decimal(18,2)" } },
    { "id": "db:Orders.Id", "stage": "db", "before": { "name": "Id", "type": "int" }, "after": { "name": "Id", "type": "int" }, "primaryKey": true },
    { "id": "dto:TipDto.amount", "stage": "dto", "after": { "name": "amount", "type": "decimal" } },
    { "id": "ui:TipForm.amount", "stage": "ui", "after": { "name": "amount", "type": "number" } } ],
  "lineage": [ { "from": "ui:TipForm.amount", "to": "db:Tips.Amount", "via": ["dto:TipDto.amount"] } ],
  "notes": [
    { "card": "db:Tips", "why": "New table: one row per tip." },
    { "card": "db:Orders", "why": "Unchanged; shown for the foreign key." } ] }
```

**icicle** — `before`/`after` are self time in ms per request (0 before = new call, 0 after = gone);
add `profiles[]` for a query or load to compare on its own tab.

```json
{ "title": "Tips at checkout",
  "calls": { "id": "checkout", "name": "CheckoutController.Pay()", "before": 3, "after": 3, "children": [
    { "id": "checkout.total", "name": "TotalCalculator.Sum()", "before": 12, "after": 4 },
    { "id": "checkout.tip", "name": "TipService.Add()", "after": 2 } ] },
  "profiles": [ { "title": "Tips report, 10k orders", "calls": { "id": "q", "name": "SELECT tips by day", "before": 180, "after": 35 } } ] }
```

## 8. Validate

```pwsh
$r = $env:RAIN_EXE   # or the installed Rain binary
& $r overview validate <abs path to overview.json>
```

It prints `valid: …` plus `warning:` lines, or every issue at once as `<path>: <message> [<CODE>]` (and
exits 1). Fix every issue and run it again. **Never fix a failure by deleting the element it names.**
Warnings (`WORKTREE` head, a node with no files, a step with no `stage`) are fine. Rain supports every
round-2 field (`risk`, `notes`, `indexes`), so there is no capability probe.

Round-2 warnings (fine to ship, but read each one): `readingOrder[i]: n of m review files have no risk
(first: …)` → rate them; `readingOrder[i].review[j] ('<path>') is rated low|medium but the path looks
domain-critical` → re-check against §5a; `notes: n notes for repo '<id>' (guide budget is 8)` → cut to
the best 8; `graph.dataModel.tables[i].indexes[j].columns[k] ('X') is not a column of the table` → add the
column or drop it from the index. `valid:` ends with `n notes, m of k review files rated`, and m < k
means some files are unrated.

**Then look at the pictures.** `valid:` says nothing about layout. Run
`& $r overview render <abs path> --out <abs dir>`. It draws every view headless with the same renderer
the window's Map uses and prints one PNG path per view and theme (`NN-<view>-dark.png` / `-light.png`).
Open the dark PNGs, fix anything §4a rules out, and render again until each picture is clean.
`render` draws the graph's views only; the animated diagrams show in the window's Map.

`valid:` does not mean the paths are real. The validator checks the `<repoId>/` prefix, not that the
file is in the diff — a placeholder like `api/src/.../placeholder.cs` passes. Before pushing, check
every graph path and file cite against the numstat file list.

| Code | What you did |
|---|---|
| `BROKEN_REFERENCE` | an edge, flow step, view, walkthrough step or phase names an id you never declared (in a diagram: an entity, shape or field it doesn't have) |
| `INVALID_DOCUMENT` / `INVALID_OVERVIEW` | an invented field (schemas are strict), a bad enum (`risk` outside `low`/`medium`/`high`/`critical`), a length over its cap (note `text` > 600), a reading-order path with a repo prefix, or a note whose `endLine` is before its `startLine`; |
| `DUPLICATE_ID` | two nodes, edges, views, repos, phases or notes share an id, or two indexes of one table share a name |
| `INVALID_DIAGRAM` | a `diagrams.<kind>` document breaks its schema (path down to the field), an unknown kind, a path with no steps, an icicle with no tree |
| `UNSUPPORTED_SCHEMA_VERSION` | `schemaVersion` is not `0.3.0` |
| `UNKNOWN_REPO_PREFIX` | a graph file path, index file path, file cite or `notes[].path` without a known `<repoId>/` prefix |
| `UNKNOWN_REPO` | a `readingOrder[].repo` not in `repos` |
| `NO_RENDER_TARGETS` | no declared lens has anything to draw: add views, flows, a `dataModel`, or the `architecture` lens |
| `UNKNOWN_VIEW` / `LENS_NOT_DECLARED` / `NOTHING_TO_RENDER` / `NO_FLOW_IN_SCOPE` / `TOO_MANY_ASSETS` | a view or flow the renderer cannot draw (wrong lens, empty scope, a data-flow view with no flow) |

## 9. Size budget

≤ 40 nodes, ≤ 60 edges, ≤ 3 flows of ≤ 5 participants, ≤ 8 views, ≤ 6 walkthrough steps, labels
≤ 20 chars, ≤ 8 notes per repo of ≤ 600 chars, every review file rated, indexes only on changed tables
(plus their unchanged neighbours when a diffed file shows them), diagrams inside their §7a budgets —
unless the story is exceptional, and then say why in the summary.

## 10. A complete example

Two repos, three lanes, a hero edge, one flow with its own view, a three-step walkthrough, a rated
reading order, two phases and three intent notes (one with "Likely …"). It predates the §4 label rule:
`POST /api/invoices/{id}/tip` (27 chars) would be `Tip endpoint` with the route in `subtitle` today.
Run it through `overview validate` (expect one `WORKTREE` warning) and `overview render` to see the Map.

```json
{
  "format": "difit-overview/v1",
  "title": "PROJ-0000 Tips on invoices (sample)",
  "repos": [
    { "id": "api", "dir": "acme-api", "base": "1111111111111111111111111111111111111111", "head": "2222222222222222222222222222222222222222" },
    { "id": "web", "dir": "acme-web", "base": "3333333333333333333333333333333333333333", "head": "WORKTREE" }
  ],
  "graph": {
    "schemaVersion": "0.3.0",
    "kind": "graph",
    "title": "Tips on invoices",
    "summary": "Adds an optional tip to invoice payments: a new TipsController action, a TipService that validates and records the tip, a Tip entity, and a tip form wired into the web app's invoice page.",
    "lenses": ["architecture", "data-flow"],
    "provenance": {
      "repo": { "owner": "acme", "name": "acme-api" },
      "base": { "sha": "1111111111111111111111111111111111111111" },
      "head": { "sha": "2222222222222222222222222222222222222222" }
    },
    "lanes": [
      { "id": "web", "label": "Web app", "subtitle": "Browser", "order": 0 },
      { "id": "api", "label": "API", "subtitle": "Backend", "order": 1 },
      { "id": "data", "label": "Data", "subtitle": "Database", "order": 2 }
    ],
    "nodes": [
      { "id": "invoice-page", "label": "Invoice page", "kind": "ui", "delta": "unchanged", "lane": "web", "files": [{ "path": "web/src/app/invoice/invoice-page.component.ts" }] },
      { "id": "tip-form", "label": "TipFormComponent", "kind": "ui", "delta": "added", "lane": "web", "files": [{ "path": "web/src/app/tips/tip-form.component.ts", "startLine": 1, "endLine": 40 }] },
      { "id": "tip-client", "label": "TipService (web)", "kind": "service", "delta": "added", "lane": "web", "files": [{ "path": "web/src/app/tips/tip.service.ts", "startLine": 8, "endLine": 20 }] },
      { "id": "tips-controller", "label": "POST /api/invoices/{id}/tip", "kind": "route", "delta": "added", "lane": "api", "files": [{ "path": "api/src/Api/TipsController.cs", "startLine": 12, "endLine": 30 }] },
      { "id": "tip-service", "label": "TipService", "kind": "service", "delta": "added", "lane": "api", "files": [{ "path": "api/src/Services/TipService.cs", "startLine": 9, "endLine": 34 }] },
      { "id": "invoice-service", "label": "InvoiceService", "kind": "service", "delta": "modified", "lane": "api", "files": [{ "path": "api/src/Services/InvoiceService.cs", "startLine": 40, "endLine": 52 }] },
      { "id": "tip-table", "label": "Tip", "kind": "datastore", "delta": "added", "lane": "data", "files": [{ "path": "api/src/Data/Tip.cs", "startLine": 1, "endLine": 18 }] }
    ],
    "edges": [
      { "id": "page-to-form", "from": "invoice-page", "to": "tip-form", "kind": "render", "delta": "added" },
      { "id": "form-to-client", "from": "tip-form", "to": "tip-client", "kind": "call", "delta": "added" },
      { "id": "client-to-api", "from": "tip-client", "to": "tips-controller", "kind": "http", "delta": "added", "label": "POST tip", "emphasis": "hero", "animated": true },
      { "id": "controller-to-service", "from": "tips-controller", "to": "tip-service", "kind": "call", "delta": "added" },
      { "id": "service-to-invoice", "from": "tip-service", "to": "invoice-service", "kind": "call", "delta": "modified" },
      { "id": "service-to-table", "from": "tip-service", "to": "tip-table", "kind": "data", "delta": "added" }
    ],
    "flows": [
      {
        "id": "add-tip",
        "title": "Adding a tip",
        "delta": "added",
        "participants": [{ "node": "tip-form" }, { "node": "tips-controller" }, { "node": "tip-service" }, { "node": "tip-table" }],
        "messages": [
          { "id": "submit", "from": "tip-form", "to": "tips-controller", "label": "POST /api/invoices/{id}/tip", "kind": "sync", "delta": "added", "files": [{ "path": "web/src/app/tips/tip.service.ts", "startLine": 14 }] },
          { "id": "record", "from": "tips-controller", "to": "tip-service", "label": "RecordTip(invoiceId, amount)", "kind": "sync", "delta": "added", "files": [{ "path": "api/src/Api/TipsController.cs", "startLine": 22 }] },
          { "id": "insert", "from": "tip-service", "to": "tip-table", "label": "INSERT Tip", "kind": "sync", "delta": "added", "files": [{ "path": "api/src/Services/TipService.cs", "startLine": 27 }] },
          { "id": "ok", "from": "tips-controller", "to": "tip-form", "label": "200 TipViewModel", "kind": "return", "delta": "added" }
        ]
      }
    ],
    "views": [
      { "id": "overview", "title": "Tips end to end", "lens": "architecture", "defaultOpen": true, "scope": { "kind": "all" } },
      { "id": "api-only", "title": "Inside the API", "lens": "architecture", "scope": { "kind": "selection", "nodes": ["tips-controller", "tip-service", "invoice-service", "tip-table"] } },
      { "id": "add-tip-flow", "title": "Adding a tip", "lens": "data-flow", "scope": { "kind": "selection", "flows": ["add-tip"] } }
    ],
    "walkthrough": {
      "steps": [
        {
          "id": "new-endpoint",
          "heading": "Invoices now take a tip",
          "body": "A new endpoint records a tip against an invoice; the web app posts it from a new form.",
          "stage": { "kind": "view", "view": "overview" },
          "focus": { "kind": "selection", "nodes": ["tips-controller", "tip-form"] },
          "detail": {
            "text": "TipFormComponent posts to the new tip endpoint, which hands the amount to TipService.",
            "cites": [
              { "text": "TipFormComponent", "ref": { "kind": "node", "node": "tip-form" } },
              { "text": "tip endpoint", "ref": { "kind": "node", "node": "tips-controller" } },
              { "text": "TipService", "ref": { "kind": "node", "node": "tip-service" } }
            ]
          }
        },
        {
          "id": "flow",
          "heading": "One request, one row",
          "body": "The tip is validated, then written as one Tip row before the invoice total is refreshed.",
          "stage": { "kind": "flow", "flow": "add-tip" },
          "focus": { "kind": "selection", "messages": ["record", "insert"] }
        },
        {
          "id": "blast",
          "heading": "4 parts added, 1 changed",
          "body": "InvoiceService is the only existing code touched; it now includes tips in the total.",
          "stage": { "kind": "view", "view": "overview" },
          "focus": { "kind": "selection", "nodes": ["invoice-service"] }
        }
      ]
    }
  },
  "readingOrder": [
    {
      "repo": "api",
      "review": [
        { "path": "src/Api/TipsController.cs", "why": "entry point", "risk": "critical" },
        { "path": "src/Services/TipService.cs", "why": "validation and the write", "risk": "high" },
        { "path": "src/Services/InvoiceService.cs", "why": "the one modified path", "risk": "high" },
        { "path": "src/Data/Tip.cs", "why": "new entity", "risk": "medium" }
      ],
      "ignore": [{ "glob": "src/Migrations/*.Designer.cs", "reason": "generated" }]
    },
    {
      "repo": "web",
      "review": [
        { "path": "src/app/tips/tip-form.component.ts", "why": "new form", "risk": "medium" },
        { "path": "src/app/tips/tip.service.ts", "why": "the call", "risk": "medium" }
      ],
      "ignore": [{ "glob": "src/api-client/**", "reason": "generated client" }]
    }
  ],
  "phases": [
    { "id": "backend", "title": "API records a tip", "status": "done", "today": "Invoices have no tip; payments carry the invoice total only.", "proposed": "A tip endpoint validates and stores a Tip row and the invoice total includes it.", "nodeIds": ["tips-controller", "tip-service", "tip-table", "invoice-service"] },
    { "id": "ui", "title": "Web app asks for a tip", "status": "pending", "today": "The invoice page has no tip control.", "proposed": "A tip form on the invoice page posts the tip before payment.", "nodeIds": ["tip-form", "tip-client", "invoice-page"] }
  ],
  "notes": [
    { "id": "record-before-refresh", "path": "api/src/Api/TipsController.cs", "startLine": 22, "text": "RecordTip runs before the invoice total refresh so the refreshed total already includes the new tip. Swapping the two calls would show the old total until the next reload." },
    { "id": "single-insert", "path": "api/src/Services/TipService.cs", "startLine": 27, "text": "The tip is written as one row in the same unit of work as the validation. Likely kept in one transaction so a rejected amount never leaves a partial Tip behind." },
    { "id": "total-includes-tips", "path": "api/src/Services/InvoiceService.cs", "startLine": 44, "endLine": 52, "text": "Existing invoices have no Tip rows, so adding tips to the total leaves their totals unchanged." }
  ]
}
```

For a `dataModel` with `files` and `indexes`, see the Rain repo's
[`fixtures/overview/sample-overview-schema.json`](../../../fixtures/overview/sample-overview-schema.json).

To push it: `rain-story.ps1 -Mode open … -Overview <abs path>`, or `Rain overview set <abs path> --session <s>`
on an open window (see [`../SKILL.md`](../SKILL.md)).
