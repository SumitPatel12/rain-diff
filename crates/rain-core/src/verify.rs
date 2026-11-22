//! The Verify by hand checklist: the Brief's Verify items with the reviewer's own edits laid over
//! them, and how the floating card was left (open, where it was dragged).
//!
//! The edits are kept apart from the Brief so an agent pushing a new one never drops them: the
//! list is the new Brief's items, each replaced by the reviewer's rewording of it and minus the
//! ones they removed, then rewordings of items the new Brief no longer has, then the reviewer's
//! own items.

use serde::{Deserialize, Serialize};

use crate::brief::BriefDoc;
use crate::session::Session;

/// The checklist items in a Verify body: its list items, or failing those its paragraphs, with
/// list markers, task boxes and emphasis marks taken off.
pub fn checklist_items(md: &str) -> Vec<String> {
    let clean = |s: &str| s.replace("**", "").replace('`', "").trim().to_owned();
    let marker = |t: &str| -> Option<String> {
        let t = t.trim_start();
        let rest = if let Some(r) = t.strip_prefix(['-', '*', '+']) {
            r
        } else {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 || digits > 3 {
                return None;
            }
            t[digits..].strip_prefix(['.', ')'])?
        };
        let rest = rest.strip_prefix(' ')?;
        let rest = ["[ ] ", "[x] ", "[X] "].iter().find_map(|b| rest.strip_prefix(b)).unwrap_or(rest);
        Some(clean(rest))
    };
    let listed: Vec<String> = md.lines().filter_map(marker).filter(|s| !s.is_empty()).collect();
    if !listed.is_empty() {
        return listed;
    }
    md.split("\n\n")
        .map(|p| clean(&p.replace('\n', " ")))
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn brief_items(doc: Option<&BriefDoc>) -> Vec<String> {
    doc.and_then(|b| b.verify.as_ref())
        .map(|v| checklist_items(&v.text))
        .unwrap_or_default()
}

/// A Brief item the reviewer reworded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VerifyEdit {
    /// The Brief's text.
    pub from: String,
    pub text: String,
}

/// The reviewer's side of the checklist, saved in the session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VerifyList {
    /// Expanded; the card starts folded to its header.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    /// The card's top-left in window points; `None` keeps it at the reader's bottom left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub edited: Vec<VerifyEdit>,
    /// Brief items the reviewer removed, by the Brief's text.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemSource {
    Brief,
    Edited,
    Added,
}

/// Which entry an item comes from, for editing or removing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemRef {
    /// A Brief item, by the Brief's text (reworded or not).
    Brief(String),
    /// The reviewer's own item, by its place in `added`.
    Added(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyItem {
    pub text: String,
    pub source: ItemSource,
    pub of: ItemRef,
}

impl VerifyList {
    /// The list the reviewer sees over the Brief's `brief` items.
    pub fn items(&self, brief: &[String]) -> Vec<VerifyItem> {
        let edit_of = |b: &str| self.edited.iter().find(|e| e.from == b);
        let mut out: Vec<VerifyItem> = brief
            .iter()
            .filter(|b| !self.removed.contains(b))
            .map(|b| match edit_of(b) {
                Some(e) => VerifyItem {
                    text: e.text.clone(),
                    source: ItemSource::Edited,
                    of: ItemRef::Brief(b.clone()),
                },
                None => VerifyItem {
                    text: b.clone(),
                    source: ItemSource::Brief,
                    of: ItemRef::Brief(b.clone()),
                },
            })
            .collect();
        out.extend(
            self.edited
                .iter()
                .filter(|e| !brief.contains(&e.from) && !self.removed.contains(&e.from))
                .map(|e| VerifyItem {
                    text: e.text.clone(),
                    source: ItemSource::Edited,
                    of: ItemRef::Brief(e.from.clone()),
                }),
        );
        out.extend(self.added.iter().enumerate().map(|(i, t)| VerifyItem {
            text: t.clone(),
            source: ItemSource::Added,
            of: ItemRef::Added(i),
        }));
        out
    }

    fn rewrite(&mut self, of: &ItemRef, text: &str) {
        match of {
            ItemRef::Brief(from) => {
                self.edited.retain(|e| e.from != *from);
                if text != from {
                    self.edited.push(VerifyEdit {
                        from: from.clone(),
                        text: text.to_owned(),
                    });
                }
            }
            ItemRef::Added(i) => {
                if let Some(t) = self.added.get_mut(*i) {
                    *t = text.to_owned();
                }
            }
        }
    }

    fn drop_item(&mut self, of: &ItemRef) {
        match of {
            ItemRef::Brief(from) => {
                self.edited.retain(|e| e.from != *from);
                if !self.removed.contains(from) {
                    self.removed.push(from.clone());
                }
            }
            ItemRef::Added(i) => {
                if *i < self.added.len() {
                    self.added.remove(*i);
                }
            }
        }
    }
}

/// Why a checklist change was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyRefusal {
    Empty,
    Duplicate,
    Missing,
}

impl Session {
    /// The checklist as the reviewer sees it: the Brief's items with their edits on top.
    pub fn verify_items(&self) -> Vec<VerifyItem> {
        self.verify.items(&brief_items(self.brief.as_ref()))
    }

    fn verify_text_taken(&self, text: &str, except: Option<&ItemRef>) -> bool {
        self.verify_items().iter().any(|i| i.text == text && Some(&i.of) != except)
    }

    pub fn add_verify(&mut self, text: &str) -> Result<(), VerifyRefusal> {
        let text = text.trim();
        if text.is_empty() {
            return Err(VerifyRefusal::Empty);
        }
        if self.verify_text_taken(text, None) {
            return Err(VerifyRefusal::Duplicate);
        }
        self.verify.added.push(text.to_owned());
        self.version += 1;
        Ok(())
    }

    /// Rewords item `of`; its tick follows the new text.
    pub fn edit_verify(&mut self, of: &ItemRef, text: &str) -> Result<(), VerifyRefusal> {
        let text = text.trim();
        if text.is_empty() {
            return Err(VerifyRefusal::Empty);
        }
        let Some(old) = self.verify_items().into_iter().find(|i| i.of == *of).map(|i| i.text) else {
            return Err(VerifyRefusal::Missing);
        };
        if old == text {
            return Ok(());
        }
        if self.verify_text_taken(text, Some(of)) {
            return Err(VerifyRefusal::Duplicate);
        }
        self.verify.rewrite(of, text);
        if let Some(v) = self.verified.iter_mut().find(|v| **v == old) {
            *v = text.to_owned();
        }
        self.version += 1;
        Ok(())
    }

    /// Removes item `of` and its tick.
    pub fn remove_verify(&mut self, of: &ItemRef) -> Result<(), VerifyRefusal> {
        let Some(old) = self.verify_items().into_iter().find(|i| i.of == *of).map(|i| i.text) else {
            return Err(VerifyRefusal::Missing);
        };
        self.verify.drop_item(of);
        self.verified.retain(|v| *v != old);
        self.version += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief::Section;

    #[test]
    fn list_items_become_the_checklist() {
        let md = "Run these:\n\n- Open **the** app\n2. Add a `voucher`\n- [x] Check the total\n";
        assert_eq!(checklist_items(md), ["Open the app", "Add a voucher", "Check the total"]);
    }

    #[test]
    fn without_a_list_each_paragraph_is_an_item() {
        assert_eq!(
            checklist_items("Open the app.\n\nAdd a voucher\nthen save."),
            ["Open the app.", "Add a voucher then save."]
        );
    }

    fn with_brief(s: &mut Session, md: &str) {
        s.brief = Some(BriefDoc {
            title: "t".into(),
            verify: Some(Section {
                id: "verify".into(),
                text: md.into(),
                ..Section::default()
            }),
            ..BriefDoc::default()
        });
    }

    fn texts(s: &Session) -> Vec<String> {
        s.verify_items().into_iter().map(|i| i.text).collect()
    }

    #[test]
    fn edits_survive_a_new_brief_and_ticks_follow_them() {
        let mut s = Session::default();
        with_brief(&mut s, "- a\n- b\n- c\n");
        s.verified.push("a".into());
        s.edit_verify(&ItemRef::Brief("a".into()), "a, then save").unwrap();
        s.edit_verify(&ItemRef::Brief("c".into()), "c on mobile").unwrap();
        s.remove_verify(&ItemRef::Brief("b".into())).unwrap();
        s.add_verify("mine").unwrap();
        assert_eq!(texts(&s), ["a, then save", "c on mobile", "mine"]);
        assert_eq!(s.verified, ["a, then save"]);
        assert_eq!(s.add_verify("mine"), Err(VerifyRefusal::Duplicate));
        assert_eq!(s.add_verify("  "), Err(VerifyRefusal::Empty));

        with_brief(&mut s, "- a\n- b\n- d\n");
        assert_eq!(
            texts(&s),
            ["a, then save", "d", "c on mobile", "mine"],
            "new items join, removals hold, an edit of a dropped item stays"
        );
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), s);

        s.edit_verify(&ItemRef::Added(0), "mine too").unwrap();
        s.remove_verify(&ItemRef::Brief("c".into())).unwrap();
        assert_eq!(texts(&s), ["a, then save", "d", "mine too"]);
    }
}
