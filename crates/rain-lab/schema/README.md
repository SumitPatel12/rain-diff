# Playground schemas

The Diagram playground draws one change four ways. Each style reads one JSON document, and each
document follows one of the schemas here. The JSON Schema files (draft 2020-12) are the contract;
the Rust types in `src/schema` read them with serde, and a test keeps the two in step (see
[Checks](#checks)).

| Schema | File | Rust type | Read by | Sample |
|---|---|---|---|---|
| A, trace and tour | `a.schema.json` | `schema::trace::Trace` | 1, Linked-text tour | `samples/a.json` |
| S, scene script | `s.schema.json` | `schema::scene::Scene` | 2, Flowchart | `samples/s.json` |
| B, fields before and after | `b.schema.json` | `schema::state::States` | 3, Field lineage and ER | `samples/b.json` |
| C, call trees before and after | `c.schema.json` | `schema::calls::CallTrees` | 4, Differential icicle | `samples/c.json` |

Shared rules:

- Keys are camelCase where they have two words (`primaryKey`); enum values are lowercase.
- A key shown with a default may be left out. An optional value may also be `null`.
- Unknown keys are refused by the schemas. The Rust readers ignore them, so check a document
  against its schema before trusting it.
- Ids are free text but must be unique where the field says so; other fields point at them.
- `[[id]]` inside prose links a name to the thing it names: hovering the link lights it.
- A status is one of `added`, `changed`, `removed`, `same`.

## A: trace and tour

What happens, in order: the components a request calls, the paths it takes through them, and a
tour whose prose links back to them. The tour draws each path as a coloured line, each component
as a station grouped into frontend, backend and database bands, and walks the tour steps one at a
time.

| Field | Type | Meaning |
|---|---|---|
| `title` | string, required | The change's name. |
| `entities[]` | array, required | Every component, function, table and data object the paths name. |
| `entities[].id` | string, required | Unique; steps, tour text and focus lists name it. |
| `entities[].label` | string, required | The name drawn for it. |
| `entities[].layer` | string, required | `web` or `ui` (frontend), `api` or `domain` (backend), `data` or `db` (database); anything else is drawn as Other. |
| `entities[].parent` | string | The entity it sits inside, such as a component's page; drawn inside the parent's box. |
| `entities[].fields[]` | strings, default `[]` | For a data object: the fields worth naming when a call carries it. |
| `paths[]` | array, required | Each request path. |
| `paths[].id` | string, required | Unique; tour text can link it. |
| `paths[].name` | string, required | What the path does; its line's name. |
| `paths[].steps[]` | array, required | The calls the path makes, in calling order. |
| `steps[].from`, `steps[].to` | string, required | The calling and the called entity's ids. |
| `steps[].call` | string, required | What the call says, such as `RecordVoucher(id, amount)`. |
| `steps[].carries[]` | strings, default `[]` | Ids of the data objects travelling with the call; the packet is labelled with them. |
| `steps[].fragment` | `alt`, `loop` or `opt` | The call runs conditionally or repeatedly; tagged on its stretch. |
| `steps[].status` | status, required | What the change did to the call. |
| `tour[]` | array, default `[]` | The tour's steps, read in order. |
| `tour[].title` | string, required | The step's heading. |
| `tour[].text` | string, required | One or two sentences; `[[id]]` links an entity or a path. |
| `tour[].focus[]` | strings, default `[]` | Entity ids the camera frames and lights while the step plays. |
| `tour[].camera.zoom` | number | The most the camera zooms in for this step (1 is the whole map). |

```json
{
  "title": "Vouchers at checkout",
  "entities": [
    { "id": "form", "label": "VoucherForm", "layer": "web" },
    { "id": "api", "label": "VouchersController", "layer": "api" },
    { "id": "dto", "label": "VoucherDto", "layer": "api", "fields": ["amount"] }
  ],
  "paths": [
    { "id": "record", "name": "Record a voucher", "steps": [
      { "from": "form", "to": "api", "call": "POST /vouchers", "carries": ["dto"], "status": "added" }
    ] }
  ],
  "tour": [
    { "title": "A form posts it", "text": "[[form]] posts to [[api]].", "focus": ["form", "api"] }
  ]
}
```

## S: scene script

A hand-written explanation as a timeline, in the manner of Manim: shapes placed in the scene's own
points, beats that act on them at given seconds, and camera keys. The flowchart plays it, shows
each beat's sentence as a card under the figure, and sums the change up from the shapes' tones.

| Field | Type | Meaning |
|---|---|---|
| `title` | string, required | What the scene explains. |
| `size` | `[w, h]`, required | The scene's own width and height, in points. |
| `shapes[]` | array, required | Every shape, hidden until a beat brings it in. |
| `shapes[].id` | string, required | Unique; actions and `[[id]]` links name it. |
| `shapes[].kind` | `box`, `text`, `path`, `dot` or `arrow`, required | A component, a caption or note, a line, a travelling value, a call. |
| `shapes[].at` | `[x, y]`, required | Top-left for boxes and text, the start for paths and arrows, the centre for dots. |
| `shapes[].props` | object, default `{}` | What the kind needs, below. |
| `props.w`, `props.h` | number | A box's size. |
| `props.r` | number | A dot's radius. |
| `props.text` | string | A box's or text's words, an arrow's call, a dot's value. |
| `props.points[]` | `[x, y]` list, default `[]` | A path's or arrow's points. |
| `props.tone` | status | Picks the colour and a box's tag; absent means `same`. |
| `beats[]` | array, required | The timeline, in order of `t`. |
| `beats[].t` | number, required | Seconds from the start. |
| `beats[].actions[]` | array, required | What happens at `t`, all at once. |
| `beats[].say` | string | The beat's sentence; `[[id]]` names a shape. Without it the card lists the actions. |
| `actions[].do` | verb, required | `create` grows it in, `write` draws or types it on, `transform` changes size, `move` travels to `to`, `fade` takes opacity from `to[0]` to `to[1]`, `focus` frames it and dims the rest (empty target ends focus), `pulse` runs a dot along it, `morph` relabels it `into`. |
| `actions[].target` | string, required | A shape's id; empty only for `focus`. |
| `actions[].to` | `[a, b]` | `move`: where to; `fade`: opacity from and to. |
| `actions[].into` | string | `morph`: the new label; after ` · `, what it now does. |
| `actions[].dur` | number, default 0.5 | Seconds the action takes. |
| `camera[]` | array, default `[]` | Camera keys: `t`, `center` (`[x, y]` in scene points) and `zoom` (1 frames the scene), all required. |

```json
{
  "title": "A voucher's journey",
  "size": [600, 200],
  "shapes": [
    { "id": "form", "kind": "box", "at": [40, 60], "props": { "w": 160, "h": 50, "text": "VoucherForm", "tone": "added" } },
    { "id": "api", "kind": "box", "at": [400, 60], "props": { "w": 160, "h": 50, "text": "VouchersController", "tone": "added" } },
    { "id": "post", "kind": "arrow", "at": [200, 85], "props": { "points": [[200, 85], [400, 85]], "text": "POST", "tone": "added" } }
  ],
  "beats": [
    { "t": 0, "actions": [{ "do": "create", "target": "form" }], "say": "Staff open the new [[form]]." },
    { "t": 1.5, "actions": [{ "do": "write", "target": "post" }, { "do": "create", "target": "api" }], "say": "It posts to [[api]]." }
  ]
}
```

## B: fields before and after

The data fields a change touches at each stage of their trip, each before and after, how values
flow between them, and why each card of fields is there. The lineage draws a column per stage
(table, entity, DTO, screen), a card per owner, a row per field, and a strand per value.

| Field | Type | Meaning |
|---|---|---|
| `title` | string, required | The change's name. |
| `fields[]` | array, default `[]` | Every field, in the order cards and rows are listed. |
| `fields[].id` | string, required | `stage:Owner.name`, such as `db:Vouchers.Amount`; the owner names the card. |
| `fields[].stage` | `db`, `entity`, `dto` or `ui`, required | A table column, an entity property, a DTO field or a screen control. |
| `fields[].before`, `fields[].after` | field state or `null` | The field on each side; a missing side means added or removed, differing sides mean changed. |
| `fields[].primaryKey` | boolean, default `false` | It is its owner's primary key. |
| `fields[].references` | string | For a foreign key: the key field's id. |
| `fields[].why` | string | What the field is for; for a new or changed one, why. Shown on hover. |
| field state `name` | string, required | The field's name at this stage. |
| field state `type` | string, required | Its type as the code writes it. |
| field state `nullable` | boolean, default `false` | It may be empty; shown as a trailing `?`. |
| `lineage[]` | array, default `[]` | Each value's way through the stages. |
| `lineage[].from`, `lineage[].to` | string, required | The source field and the field where the value ends. |
| `lineage[].via[]` | strings, default `[]` | The fields between, in order. |
| `notes[]` | array, default `[]` | One per card. |
| `notes[].card` | string, required | `stage:Owner`, such as `db:Vouchers`. |
| `notes[].why` | string, required | Why the card is there; for a new card, what it adds. |

```json
{
  "title": "Vouchers at checkout",
  "fields": [
    { "id": "db:Vouchers.Amount", "stage": "db", "before": null, "after": { "name": "Amount", "type": "decimal(18,2)" } },
    { "id": "ui:VoucherForm.amount", "stage": "ui", "before": null, "after": { "name": "amount", "type": "number" } }
  ],
  "lineage": [{ "from": "db:Vouchers.Amount", "to": "ui:VoucherForm.amount" }],
  "notes": [{ "card": "db:Vouchers", "why": "New: one row per voucher." }]
}
```

## C: call trees before and after

Where the time goes: call trees whose frames carry their own work before and after the change.
The icicle hangs each tree from its root, bars as wide as their work, red where it grew; a tab per
tree.

| Field | Type | Meaning |
|---|---|---|
| `title` | string, required | The change's name. |
| `calls` | call frame | The tree under the change's entry points; the first tab. |
| `profiles[]` | array, default `[]` | More trees to compare, one tab each. |
| `profiles[].title` | string, required | What was measured and under what load; the tab's label. |
| `profiles[].calls` | call frame, required | Its tree. |
| frame `id` | string, required | Unique across every tree in the document. |
| frame `name` | string, required | The frame's label. |
| frame `before`, `after` | number, default 0 | Its own work (self time) in ms per request; 0 before is a new call, 0 after a call that is gone. |
| frame `children[]` | frames, default `[]` | The calls it makes. |

```json
{
  "title": "Vouchers at checkout",
  "calls": {
    "id": "pay", "name": "PaymentsController.Charge()", "before": 2, "after": 2,
    "children": [
      { "id": "pay.compute", "name": "TotalCalculator.Compute()", "before": 10, "after": 0 },
      { "id": "pay.total", "name": "AccountService.Total()", "after": 6 }
    ]
  }
}
```

## Checks

`cargo test -p rain-lab schema` runs them:

- every sample validates against its schema, and so does the sample after a serde round trip;
- for each schema, a document with every property set is read by the Rust type and written back
  unchanged, so neither side has a field the other lacks;
- leaving a property out fails to parse exactly when the schema marks it required;
- every enum value in a schema parses, and one outside it does not.

The validator is a small one in the tests (types, properties, required, additionalProperties,
items, minItems and maxItems, enum, anyOf, `$ref` into `$defs`); it covers what these schemas use.
