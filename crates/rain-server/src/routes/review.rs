//! `GET /api/review`, `GET /api/review/:n`, `POST /api/review`: finished review rounds.

use rain_core::session::ThreadOutcome;
use rain_core::{EventKind, ReviewRound, Session, Verdict};
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, error, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Get, "/api/review") => {
            let s = ctx.shared.lock();
            value(200, &json!({ "round": s.round(), "rounds": s.rounds }))
        }
        (Method::Get, p) if p.starts_with("/api/review/") => {
            let n = p["/api/review/".len()..].parse::<u32>().ok();
            let s = ctx.shared.lock();
            match n.and_then(|n| s.rounds.iter().find(|r| r.n == n)) {
                Some(round) => value(200, round),
                None => error(404, &format!("No review round {}", &p["/api/review/".len()..])),
            }
        }
        (Method::Post, "/api/review") => post(ctx, r.body),
        _ => return None,
    })
}

/// The round a submit records: the verdict and summary given, and the session as it is now.
pub(crate) fn round_now(s: &Session, verdict: Verdict, summary: String) -> ReviewRound {
    ReviewRound {
        n: s.round(),
        at: rain_comments::clock::now_iso(),
        verdict,
        summary,
        shas: s.repos.clone(),
        viewed: s.viewed.clone(),
        dispositions: s
            .threads
            .iter()
            .filter(|t| t.disposition.is_some() || !t.is_open())
            .map(|t| ThreadOutcome {
                thread: t.id.clone(),
                disposition: t.disposition,
                state: t.state,
                dismiss_reason: t.dismiss_reason,
            })
            .collect(),
        questions_open: s
            .threads
            .iter()
            .filter(|t| t.is_open() && t.kind() == rain_core::ThreadKind::Question)
            .map(|t| t.id.clone())
            .collect(),
        drafts_unsent: s.drafts.len(),
        accepted: s.accepted.clone(),
    }
}

/// `{"verdict","summary"?,"target"?}` → appends a round; Finish review from the API. `target`
/// is the sha the reviewer read: 409 when the session has loaded another since, as the dialog
/// refuses after a reload.
fn post(ctx: &Ctx, body: &str) -> Reply {
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid review data");
    };
    let Some(verdict) = o.get("verdict").and_then(|v| serde_json::from_value::<Verdict>(v.clone()).ok()) else {
        return error(400, "Invalid review field: verdict");
    };
    let summary = o.get("summary").and_then(Value::as_str).unwrap_or_default().to_owned();
    let expected = match o.get("target") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if !t.is_empty() => Some(t.clone()),
        Some(_) => return error(400, "Invalid review field: target"),
    };
    let mut s = ctx.shared.lock();
    if let Some(t) = expected
        && !s.repos.iter().any(|r| r.target.starts_with(&t))
    {
        let now: Vec<&str> = s.repos.iter().map(|r| r.target.as_str()).collect();
        return error(
            409,
            &format!(
                "The diff was reloaded since {t} (now at {}); read what changed and finish again",
                now.join(", ")
            ),
        );
    }
    let round = round_now(&s, verdict, summary);
    let n = round.n;
    s.rounds.push(round);
    s.version += 1;
    let saved = ctx.save(&s);
    drop(s);
    ctx.write(saved);
    push(ctx, EventKind::Submitted, None, None, json!({ "round": n, "verdict": verdict }));
    ctx.shared.notify();
    value(200, &json!({ "success": true, "round": n }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use rain_core::{Session, SessionRepo, Shared};

    use super::*;

    fn ctx() -> Ctx {
        let s = Session {
            repos: vec![SessionRepo {
                id: "server".into(),
                target: "abc1234def".into(),
                ..SessionRepo::default()
            }],
            ..Session::default()
        };
        Ctx {
            port: 0,
            shared: Arc::new(Shared::new(s)),
            session_file: None,
            repo_folder: None,
            lock: None,
        }
    }

    fn post_review(ctx: &Ctx, body: &str) -> Reply {
        let q = BTreeMap::new();
        route(
            ctx,
            &Req {
                method: &Method::Post,
                path: "/api/review",
                query: &q,
                body,
                markdown: false,
            },
        )
        .expect("routed")
    }

    #[test]
    fn a_review_of_an_older_sha_is_refused() {
        let c = ctx();
        let r = post_review(&c, r#"{"verdict":"approve","target":"0ld5ha"}"#);
        assert_eq!(r.status, 409, "{}", r.body);
        assert!(c.shared.lock().rounds.is_empty());
        assert_eq!(post_review(&c, r#"{"verdict":"approve","target":7}"#).status, 400);
        let ok = post_review(&c, r#"{"verdict":"request-changes","summary":"s","target":"abc1234"}"#);
        assert_eq!(ok.status, 200, "{}", ok.body);
        let s = c.shared.lock();
        assert_eq!((s.rounds.len(), s.rounds[0].verdict, s.round()), (1, Verdict::RequestChanges, 2));
        drop(s);
        assert_eq!(post_review(&c, r#"{"verdict":"comment"}"#).status, 200, "target is optional");
        assert_eq!(c.shared.events_since(0, &[EventKind::Submitted]).len(), 2);
    }
}
