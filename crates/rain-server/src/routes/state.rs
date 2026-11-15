//! Where the window is and where an agent points it: `GET /api/state`, `POST /api/goto`,
//! `POST /api/highlight`.

use rain_core::{Side, UiRequest};
use serde_json::{Map, Value, json};
use tiny_http::Method;

use crate::{Ctx, Reply, Req, error, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Get, "/api/state") => state(ctx),
        (Method::Post, "/api/goto") => goto(ctx, r.body),
        (Method::Post, "/api/highlight") => highlight(ctx, r.body),
        _ => return None,
    })
}

/// The window's reading state (as it last published it) plus the session's viewed files, round
/// and saved place.
fn state(ctx: &Ctx) -> Reply {
    let ui = ctx.shared.ui_state();
    let mut out = match serde_json::to_value(&ui) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    let s = ctx.shared.lock();
    let viewed: Vec<Value> = s
        .viewed
        .iter()
        .map(|v| {
            let key = if v.repo.is_empty() {
                v.file_path.clone()
            } else {
                format!("{}/{}", v.repo, v.file_path)
            };
            json!({ "key": key, "hash": v.diff_content_hash, "hunks": v.hunks })
        })
        .collect();
    out.insert("viewed".into(), Value::Array(viewed));
    out.insert("round".into(), json!(s.round()));
    out.insert("place".into(), json!(s.place));
    drop(s);
    value(200, &Value::Object(out))
}

type Fields = Map<String, Value>;

fn text<'a>(o: &'a Fields, k: &str) -> Option<&'a str> {
    o.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn side(o: &Fields, what: &str) -> Result<Side, Reply> {
    match o.get("side").and_then(Value::as_str) {
        None | Some("new") => Ok(Side::New),
        Some("old") => Ok(Side::Old),
        Some(_) => Err(error(400, &format!("Invalid {what} field: side"))),
    }
}

fn line(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|&n| n > 0)
}

/// `{"path","line"?,"side"?}` or `{"threadId"}`; the open window scrolls there.
fn goto(ctx: &Ctx, body: &str) -> Reply {
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid goto data");
    };
    let request = if let Some(id) = text(&o, "threadId") {
        if rain_comments::ops::thread(&ctx.shared.lock(), id).is_none() {
            return error(404, &format!("Thread not found: {id}"));
        }
        UiRequest::GotoThread(id.to_owned())
    } else if let Some(path) = text(&o, "path") {
        let line = match o.get("line") {
            None | Some(Value::Null) => None,
            Some(v) => match line(v) {
                Some(n) => Some(n),
                None => return error(400, "Invalid goto field: line"),
            },
        };
        let side = match side(&o, "goto") {
            Ok(s) => s,
            Err(r) => return r,
        };
        UiRequest::Goto {
            path: rain_core::paths::normalize(path),
            line,
            side,
        }
    } else {
        return error(400, "Invalid goto data: give path or threadId");
    };
    ctx.shared.push_request(request);
    value(200, &json!({ "success": true }))
}

/// A `[a, b]` pair of positive numbers with `a <= b`.
fn pair(v: Option<&Value>) -> Option<(u32, u32)> {
    let a = v?.as_array().filter(|a| a.len() == 2)?;
    let (x, y) = (line(&a[0])?, line(&a[1])?);
    (x <= y).then_some((x, y))
}

/// `{"path","lines":[a,b],"side"?,"caption"?,"step"?:[i,n]}`: the reader spotlights the range.
fn highlight(ctx: &Ctx, body: &str) -> Reply {
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid highlight data");
    };
    let Some(path) = text(&o, "path") else {
        return error(400, "Invalid highlight field: path");
    };
    let Some(lines) = pair(o.get("lines")) else {
        return error(400, "Invalid highlight field: lines");
    };
    let side = match side(&o, "highlight") {
        Ok(s) => s,
        Err(r) => return r,
    };
    let step = match o.get("step").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match pair(Some(v)) {
            Some(s) => Some(s),
            None => return error(400, "Invalid highlight field: step"),
        },
    };
    ctx.shared.push_request(UiRequest::Highlight {
        path: rain_core::paths::normalize(path),
        lines,
        side,
        caption: text(&o, "caption").map(str::to_owned),
        step,
    });
    value(200, &json!({ "success": true }))
}
