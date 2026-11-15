//! Review tags a file carries (round 2, from the overview) and the triage groups, reasons and
//! ordered items that replace them.

use serde::{Deserialize, Serialize};

use crate::overview::RiskLevel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileTag {
    MustReview,
    Review,
    Skim,
    FormattingOnly,
    Generated,
    Ignored,
    Uncovered,
}

impl FileTag {
    pub const ALL: [FileTag; 7] = [
        FileTag::MustReview,
        FileTag::Review,
        FileTag::Skim,
        FileTag::FormattingOnly,
        FileTag::Generated,
        FileTag::Ignored,
        FileTag::Uncovered,
    ];

    /// critical/high → must review, medium → review, low → skim, unrated → review.
    pub fn from_risk(r: Option<RiskLevel>) -> FileTag {
        match r {
            Some(RiskLevel::Critical | RiskLevel::High) => FileTag::MustReview,
            Some(RiskLevel::Medium) | None => FileTag::Review,
            Some(RiskLevel::Low) => FileTag::Skim,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FileTag::MustReview => "Must review",
            FileTag::Review => "Review",
            FileTag::Skim => "Skim",
            FileTag::FormattingOnly => "Formatting only",
            FileTag::Generated => "Generated",
            FileTag::Ignored => "Ignored",
            FileTag::Uncovered => "Uncovered",
        }
    }

    /// A short mark that reads without colour.
    pub fn glyph(&self) -> &'static str {
        match self {
            FileTag::MustReview => "!!",
            FileTag::Review => "!",
            FileTag::Skim => "~",
            FileTag::FormattingOnly => "ws",
            FileTag::Generated => "gen",
            FileTag::Ignored => "-",
            FileTag::Uncovered => "?",
        }
    }

    /// The triage group this tag falls in; `Uncovered` is a real change to review.
    pub fn group(&self) -> Group {
        match self {
            FileTag::MustReview => Group::Must,
            FileTag::Review | FileTag::Uncovered => Group::Review,
            FileTag::Skim => Group::Skim,
            FileTag::FormattingOnly => Group::FormattingOnly,
            FileTag::Generated => Group::Generated,
            FileTag::Ignored => Group::Ignored,
        }
    }
}

/// Where triage puts a file. The first three are real changes ranked by risk; the rest are groups
/// a reviewer can accept without reading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Must,
    #[default]
    Review,
    Skim,
    Tests,
    Moves,
    FormattingOnly,
    Generated,
    Lockfile,
    Huge,
    Binary,
    Ignored,
}

impl Group {
    pub const ALL: [Group; 11] = [
        Group::Must,
        Group::Review,
        Group::Skim,
        Group::Tests,
        Group::Moves,
        Group::FormattingOnly,
        Group::Generated,
        Group::Lockfile,
        Group::Huge,
        Group::Binary,
        Group::Ignored,
    ];

    /// Must, Review and Skim: files a reviewer reads.
    pub fn is_real(&self) -> bool {
        matches!(self, Group::Must | Group::Review | Group::Skim)
    }

    pub fn from_risk(r: Option<RiskLevel>) -> Group {
        FileTag::from_risk(r).group()
    }

    pub fn label(&self) -> &'static str {
        match self {
            Group::Must => "Must",
            Group::Review => "Review",
            Group::Skim => "Skim",
            Group::Tests => "Tests",
            Group::Moves => "Moves",
            Group::FormattingOnly => "Formatting-only",
            Group::Generated => "Generated",
            Group::Lockfile => "Lockfile",
            Group::Huge => "Huge",
            Group::Binary => "Binary",
            Group::Ignored => "Ignored",
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            Group::Must => "!!",
            Group::Review => "!",
            Group::Skim => "~",
            Group::Tests => "t",
            Group::Moves => "mv",
            Group::FormattingOnly => "ws",
            Group::Generated => "gen",
            Group::Lockfile => "lock",
            Group::Huge => "big",
            Group::Binary => "bin",
            Group::Ignored => "-",
        }
    }
}

/// Why a file is in its group. `trusted` reasons come from a rule over the diff itself
/// (`-w` empty, a path rule); untrusted ones are heuristics or the agent's word.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub group: Group,
    pub trusted: bool,
    pub text: String,
}

/// One entry of the ordered review list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `<repo>/<path>` in multi-repo sessions, else the path.
    pub key: String,
    pub group: Group,
    pub reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// A formatting change with one real line, which keeps the file out of FormattingOnly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_line: Option<u32>,
    /// Test ↔ subject key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair: Option<String>,
    /// Old key and the number of edited lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<(String, u32)>,
}

/// The shape of a change, which picks the reader mode and list state on first open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    Tiny,
    #[default]
    Dense,
    SparseHuge,
    GeneratedHuge,
    Iterative,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_map_to_groups_and_groups_serialize_kebab() {
        assert_eq!(FileTag::MustReview.group(), Group::Must);
        assert_eq!(FileTag::Uncovered.group(), Group::Review);
        assert_eq!(FileTag::FormattingOnly.group(), Group::FormattingOnly);
        assert_eq!(Group::from_risk(Some(RiskLevel::Low)), Group::Skim);
        assert_eq!(serde_json::to_string(&Group::FormattingOnly).unwrap(), r#""formatting-only""#);
        assert!(Group::ALL.iter().all(|g| !g.label().is_empty() && !g.glyph().is_empty()));
        assert_eq!(Group::ALL.iter().filter(|g| g.is_real()).count(), 3);
        let i = Item {
            key: "server/a.cs".into(),
            moved_from: Some(("server/b.cs".into(), 2)),
            ..Item::default()
        };
        let s = serde_json::to_string(&i).unwrap();
        assert_eq!(
            s,
            r#"{"key":"server/a.cs","group":"review","reason":{"group":"review","trusted":false,"text":""},"movedFrom":["server/b.cs",2]}"#
        );
        assert_eq!(serde_json::from_str::<Item>(&s).unwrap(), i);
    }

    #[test]
    fn risk_maps_to_tags() {
        assert_eq!(FileTag::from_risk(Some(RiskLevel::Critical)), FileTag::MustReview);
        assert_eq!(FileTag::from_risk(Some(RiskLevel::High)), FileTag::MustReview);
        assert_eq!(FileTag::from_risk(Some(RiskLevel::Medium)), FileTag::Review);
        assert_eq!(FileTag::from_risk(Some(RiskLevel::Low)), FileTag::Skim);
        assert_eq!(FileTag::from_risk(None), FileTag::Review);
        assert!(FileTag::ALL.iter().all(|t| !t.label().is_empty() && !t.glyph().is_empty()));
    }
}
