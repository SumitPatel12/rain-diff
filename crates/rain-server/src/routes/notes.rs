//! `GET/POST /api/notes`, `DELETE /api/notes/:id`.

use rain_comments::{add_notes, notes, remove_note};
use rain_core::notes::OVERVIEW_NOTE_PREFIX;
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, error, json_reply, percent_decode, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    let repo = r.repo();
    Some(match (r.method, r.path) {
        (Method::Get, "/api/notes") => {
            let file = r.query.get("filePath").filter(|p| !p.is_empty()).map(|p| {
                let p = rain_core::paths::normalize(p);
                repo.map_or(p.clone(), |r| rain_core::paths::join(r, &p))
            });
            json_reply(200, rain_comments::notes_json_string(&ctx.shared.lock(), repo, file.as_deref()))
        }
        (Method::Post, "/api/notes") => post_notes(ctx, repo, r.flag("replace"), r.body),
        (Method::Delete, p) if p.starts_with("/api/notes/") => delete_note(ctx, &percent_decode(&p["/api/notes/".len()..])),
        _ => return None,
    })
}

fn post_notes(ctx: &Ctx, repo: Option<&str>, replace: bool, body: &str) -> Reply {
    let parsed = serde_json::from_str::<Value>(body)
        .map_err(|_| "Invalid note JSON".to_owned())
        .and_then(notes::from_value);
    let incoming = match parsed {
        Ok(n) => n,
        Err(e) => return error(400, &e),
    };
    let author = incoming.first().and_then(|n| n.author.clone());
    let mut s = ctx.shared.lock();
    let r = add_notes(&mut s, incoming, repo.unwrap_or(""), replace);
    if r.changed {
        if ctx.shared.has_excerpt_source() {
            rain_comments::snapshot_notes(&mut s, &|r, p, a| ctx.shared.excerpt(r, p, a));
        }
        rain_comments::ops::reanchor_targets(&mut s);
        ctx.save(&s);
    }
    let version = s.notes_version;
    drop(s);
    if r.changed {
        push(
            ctx,
            rain_core::EventKind::Notes,
            author,
            None,
            json!({ "ids": r.ids, "notesVersion": version }),
        );
        ctx.shared.notify();
    }
    value(
        200,
        &json!({ "success": true, "changed": r.changed, "count": r.count, "ids": r.ids, "notesVersion": version }),
    )
}

fn delete_note(ctx: &Ctx, id: &str) -> Reply {
    if id.starts_with(OVERVIEW_NOTE_PREFIX) {
        return error(409, &format!("Note {id} comes from the overview; push a new overview to change it"));
    }
    let mut s = ctx.shared.lock();
    if !remove_note(&mut s, id) {
        return error(404, &format!("Note not found: {id}"));
    }
    rain_comments::ops::reanchor_targets(&mut s);
    ctx.save(&s);
    let version = s.notes_version;
    drop(s);
    push(
        ctx,
        rain_core::EventKind::Notes,
        None,
        None,
        json!({ "removed": id, "notesVersion": version }),
    );
    ctx.shared.notify();
    value(200, &json!({ "success": true, "id": id, "notesVersion": version }))
}
