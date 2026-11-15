//! The Brief (`rain-brief/v1`): the agent's narrative of a change, in sections with stable ids so
//! reviewers can comment on them and the comments survive a rewrite.

use serde::{Deserialize, Serialize};

use crate::diff::Side;

pub const BRIEF_FORMAT: &str = "rain-brief/v1";

/// A pointer from prose into the diff: `<repo>/<path>`, optional lines and side, optional thread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cite {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSha {
    pub repo: String,
    pub sha: String,
}

/// Intent, Not changed and Verify: one block of Markdown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    #[serde(default)]
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<Cite>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct How {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<Cite>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invariant {
    #[serde(default)]
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforced: Vec<Cite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tested: Vec<Cite>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotCovered {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: String,
    /// Globs of files the brief deliberately leaves out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TourStep {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    pub cite: Cite,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefDoc {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub written_against: Vec<RepoSha>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub how: Vec<How>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<Invariant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_changed: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_covered: Option<NotCovered>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<Section>,
    /// Empty means one step per `how[]` cite ([`BriefDoc::tour_steps`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tour: Vec<TourStep>,
}

/// A commentable section as the reader and the re-anchor rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionRef<'a> {
    pub id: &'a str,
    /// `Intent`, `How 2 · The API records a voucher`, …
    pub title: &'a str,
    pub text: &'a str,
}

impl BriefDoc {
    /// Fills missing ids: `intent`, `how.N`, `inv.N`, `not-changed`, `not-covered`, `verify`,
    /// `tour.N` (1-based), leaving ids the agent gave alone.
    pub fn assign_ids(&mut self) {
        fn fill(id: &mut String, v: String) {
            if id.is_empty() {
                *id = v;
            }
        }
        if let Some(s) = &mut self.intent {
            fill(&mut s.id, "intent".into());
        }
        for (i, h) in self.how.iter_mut().enumerate() {
            fill(&mut h.id, format!("how.{}", i + 1));
        }
        for (i, v) in self.invariants.iter_mut().enumerate() {
            fill(&mut v.id, format!("inv.{}", i + 1));
        }
        if let Some(s) = &mut self.not_changed {
            fill(&mut s.id, "not-changed".into());
        }
        if let Some(s) = &mut self.not_covered {
            fill(&mut s.id, "not-covered".into());
        }
        if let Some(s) = &mut self.verify {
            fill(&mut s.id, "verify".into());
        }
        for (i, t) in self.tour.iter_mut().enumerate() {
            fill(&mut t.id, format!("tour.{}", i + 1));
        }
    }

    /// Every commentable section in reading order.
    pub fn sections(&self) -> Vec<SectionRef<'_>> {
        let mut out = Vec::new();
        if let Some(s) = &self.intent {
            out.push(SectionRef {
                id: &s.id,
                title: "Intent",
                text: &s.text,
            });
        }
        for h in &self.how {
            out.push(SectionRef {
                id: &h.id,
                title: &h.title,
                text: &h.text,
            });
        }
        for v in &self.invariants {
            out.push(SectionRef {
                id: &v.id,
                title: "Invariant",
                text: &v.text,
            });
        }
        if let Some(s) = &self.not_changed {
            out.push(SectionRef {
                id: &s.id,
                title: "Not changed",
                text: &s.text,
            });
        }
        if let Some(s) = &self.not_covered {
            out.push(SectionRef {
                id: &s.id,
                title: "Not covered",
                text: &s.text,
            });
        }
        if let Some(s) = &self.verify {
            out.push(SectionRef {
                id: &s.id,
                title: "Verify by hand",
                text: &s.text,
            });
        }
        out
    }

    pub fn section(&self, id: &str) -> Option<SectionRef<'_>> {
        self.sections().into_iter().find(|s| s.id == id)
    }

    /// Every cite in the brief, with the id of the section it sits in.
    pub fn cites(&self) -> Vec<(&str, &Cite)> {
        let mut out: Vec<(&str, &Cite)> = Vec::new();
        for s in [&self.intent, &self.not_changed, &self.verify].into_iter().flatten() {
            out.extend(s.cites.iter().map(|c| (s.id.as_str(), c)));
        }
        for h in &self.how {
            out.extend(h.cites.iter().map(|c| (h.id.as_str(), c)));
        }
        for v in &self.invariants {
            out.extend(v.enforced.iter().chain(&v.tested).map(|c| (v.id.as_str(), c)));
        }
        out.extend(self.tour.iter().map(|t| (t.id.as_str(), &t.cite)));
        out
    }

    /// The tour as given, else one step per `how[]` cite titled by its section.
    pub fn tour_steps(&self) -> Vec<TourStep> {
        if !self.tour.is_empty() {
            return self.tour.clone();
        }
        self.how
            .iter()
            .flat_map(|h| h.cites.iter().map(move |c| (h, c)))
            .enumerate()
            .map(|(i, (h, c))| TourStep {
                id: format!("tour.{}", i + 1),
                title: h.title.clone(),
                text: h.text.clone(),
                cite: c.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../fixtures/brief/sample.json");

    #[test]
    fn sample_parses_and_reserialises() {
        let doc: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(doc.format, BRIEF_FORMAT);
        assert_eq!(doc.how.len(), 3);
        assert_eq!(doc.how[1].cites[0].lines, Some((27, 31)));
        let back = serde_json::to_value(&doc).unwrap();
        let orig: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(back, orig);
        assert_eq!(serde_json::from_value::<BriefDoc>(back).unwrap(), doc);
    }

    #[test]
    fn ids_fill_only_where_missing() {
        let mut doc: BriefDoc = serde_json::from_str(
            r#"{"format":"rain-brief/v1","title":"t","intent":{"text":"i"},"how":[{"title":"a"},{"id":"api","title":"b"},{"title":"c"}],"verify":{"text":"v"}}"#,
        )
        .unwrap();
        doc.assign_ids();
        let ids: Vec<&str> = doc.sections().iter().map(|s| s.id).collect();
        assert_eq!(ids, ["intent", "how.1", "api", "how.3", "verify"]);
        assert_eq!(doc.section("api").unwrap().title, "b");
    }

    #[test]
    fn tour_defaults_to_how_cites() {
        let mut doc: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        let n: usize = doc.how.iter().map(|h| h.cites.len()).sum();
        doc.tour.clear();
        let steps = doc.tour_steps();
        assert_eq!(steps.len(), n);
        assert_eq!(steps[0].id, "tour.1");
        assert_eq!(steps[0].cite, doc.how[0].cites[0]);
        assert!(doc.cites().iter().any(|(id, _)| *id == "how.2"));
    }
}
