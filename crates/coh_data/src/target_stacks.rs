//! `targetStacks` — the stacks of another power a TARGET carries that a power counts.
//!
//! Brainstorm's Spines poison is the case that brought this in: each hit grants the foe one
//! `Temporary_Powers.Temporary_Powers.Poison`, and the poison's damage grows with how many the
//! foe already holds (`… target.ownPowerNum? .9 * 1 + @StdResult *`). Who is being hit is a
//! per-cast fact a build cannot know, so the player states the count with a slider
//! ([`crate::CombatContext::target_stacks`]), and an unstated count is a fresh target: zero.
//!
//! The converter stamps the list (`extractTargetStacks`) because the slider's top end is the
//! counted record's own `num_allowed`, which only the export holds. Decode is `Result` for the
//! `grant_edges` reason: a shape this reader does not know is drift, not a default.

use crate::{CharacterState, Power, PowerDatabase};
use serde_json::Value;
use std::collections::BTreeMap;

/// One counted power, as the converter stamped it.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetStack {
    /// The counted power's dotted export path, verbatim — the spelling the gates read, and the
    /// key [`crate::CombatContext::target_stacks`] holds the count under.
    pub path: String,
    /// The counted record's display name (`Poison`), for the slider's label.
    pub name: String,
    /// The counted record's `num_allowed`: how many a target can hold. `None` = the record
    /// states no ceiling.
    pub max_count: Option<u32>,
}

/// The power's stamped counters. `Ok(vec![])` when it counts none.
pub fn target_stacks(power: &Power) -> Result<Vec<TargetStack>, String> {
    let Some(raw) = power.extra.get("targetStacks") else {
        return Ok(Vec::new());
    };
    let name = power.internal_name.as_deref().unwrap_or(&power.name);
    raw.as_array()
        .ok_or_else(|| format!("{name}: targetStacks is not an array"))?
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            decode(entry).map_err(|e| format!("{name} targetStacks[{index}]: {e}"))
        })
        .collect()
}

fn decode(entry: &Value) -> Result<TargetStack, String> {
    let object = entry.as_object().ok_or("not an object")?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing {key}"))
    };
    let max_count = match object.get("maxCount") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| format!("maxCount {value} is not a count"))?,
        ),
    };
    Ok(TargetStack {
        path: text("path")?,
        name: text("name")?,
        max_count,
    })
}

/// Every counter the build's picked powers read, one per counted path. Two powers counting the
/// same path agree on its record, so the first one read stands for both.
pub fn build_target_stacks(
    state: &CharacterState,
    db: &PowerDatabase,
) -> Result<Vec<TargetStack>, String> {
    let mut found: BTreeMap<String, TargetStack> = BTreeMap::new();
    for selection in state.all_selected() {
        let Some(power) = db.resolve_power(&selection.powerset, &selection.internal_name) else {
            continue;
        };
        for stack in target_stacks(power)? {
            found
                .entry(stack.path.to_ascii_lowercase())
                .or_insert(stack);
        }
    }
    Ok(found.into_values().collect())
}
