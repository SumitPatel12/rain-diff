//! Schema B: the data fields a change touches, each at one stage of its trip (table, entity,
//! DTO, screen) and before and after the change; a missing side (`null`) means added or
//! removed. Lineage says how values flow between them, notes why each card is there. Spec:
//! `schema/b.schema.json`.

use serde::{Deserialize, Serialize};

use super::Status;

pub const RAW: &str = include_str!("../../samples/b.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct States {
    pub title: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub lineage: Vec<Lineage>,
    /// Why a table, entity, DTO or screen holds the fields it does; for a new one, what it adds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
}

/// A note on one card of fields: `card` reads `stage:Owner` (`db:Vouchers`), as field ids do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub card: String,
    pub why: String,
}

/// A data field at one stage of its trip: a column, an entity property, a DTO field, a UI
/// control. Ids read `stage:Owner.name`; the owner names its card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    pub stage: Stage,
    pub before: Option<FieldState>,
    pub after: Option<FieldState>,
    /// It is its owner's primary key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub primary_key: bool,
    /// The key a foreign key points at (another field's id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
    /// What the field is for; for a new or changed one, why the change made it so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Db,
    Entity,
    Dto,
    Ui,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Db, Stage::Entity, Stage::Dto, Stage::Ui];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldState {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub nullable: bool,
}

/// One value's way from a source field to where it ends, through the fields between.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lineage {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub via: Vec<String>,
}

impl Lineage {
    /// Every field on the way, in order.
    pub fn hops(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.from.as_str())
            .chain(self.via.iter().map(String::as_str))
            .chain(std::iter::once(self.to.as_str()))
    }
}

impl Field {
    pub fn status(&self) -> Status {
        Status::of(self.before.as_ref(), self.after.as_ref())
    }
}

impl States {
    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    pub fn note(&self, card: &str) -> Option<&str> {
        self.notes.iter().find(|n| n.card == card).map(|n| n.why.as_str())
    }
}
