//! How far the target is — the `distance` reader's answer, per power.
//!
//! A handful of powers fork on it. Attune's damage and debuffs hold only within 40 ft; Savage
//! Leap and Feral Charge pay out one, two or three Blood Frenzy by the band they were used from
//! (under 20, 20–40, 40 and over) and teleport only past 7 ft; Telekinesis repels what drifts
//! under 40 ft or past 60. No build state pins any of it, so two answers stand in:
//!
//! * **The power's reach**, with no input. A target the power is hitting is somewhere between
//!   0 ft and its range (its radius, for a power with no range). That alone settles every gate
//!   whose threshold the reach does not straddle — Attune's 20 ft never reaches 40 — and leaves
//!   the rest Indeterminate, which is what they are.
//! * **A band the player picks**, for a power whose gates the reach leaves open. The bands are
//!   cut at the thresholds the power's own gates name, so every value inside one answers every
//!   gate alike, and the pick is stored as the band's midpoint
//!   ([`coh_data::SelectedPower::target_distance`]). No band is invented: a power names its own.
//!
//! The reach is the BASE range. Range enhancement can stretch a leap further than its base, and
//! the outermost band says "and beyond" for exactly that reason; the gates past the base range
//! are the ones a pick answers, not the reach.

use crate::expr::{eval_bool, SourceContext};
use crate::window_slots::window_slots;
use coh_data::grant_edges::grant_edges;
use coh_data::{AtomicEffect, DatasetId, EffectType, Power};
use serde_json::Value;
use std::borrow::Cow;

/// One choosable distance band, `[lo, hi)` in feet.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct DistanceBand {
    pub lo: f64,
    pub hi: f64,
}

impl DistanceBand {
    /// The value stored for a pick of this band. Any point strictly inside answers every gate
    /// the same way, because the bands are cut at every threshold the power names.
    pub fn midpoint(&self) -> f64 {
        (self.lo + self.hi) / 2.0
    }

    /// Whether a stored pick falls in this band.
    pub fn holds(&self, distance: f64) -> bool {
        distance >= self.lo && distance < self.hi
    }
}

/// The furthest a target this power is hitting can be, in feet: its base range, else its
/// radius, else 0 (a self power's target is its caster).
pub fn reach(power: &Power) -> f64 {
    let stat = |key: &str| {
        power
            .extra
            .get("stats")
            .and_then(|stats| stats.get(key))
            .and_then(Value::as_f64)
            .filter(|value| *value > 0.0)
    };
    stat("range").or_else(|| stat("radius")).unwrap_or(0.0)
}

/// Every threshold this power's gates compare `distance` against (`distance 40 <=` → 40), from
/// its atoms and its grant edges, ascending and deduplicated.
pub fn thresholds(power: &Power) -> Vec<f64> {
    let atom_gates = power
        .atoms
        .iter()
        .filter_map(|atom| atom.requires_expression.as_deref());
    let edges = grant_edges(power).unwrap_or_default();
    let edge_gates = edges.iter().filter_map(|edge| edge.condition.as_deref());
    let mut found: Vec<f64> = atom_gates
        .chain(edge_gates)
        .flat_map(|gate| {
            gate.windows(3)
                .filter(|w| &*w[0] == "distance" && is_comparison(&w[2]))
                .filter_map(|w| w[1].parse::<f64>().ok())
                .collect::<Vec<_>>()
        })
        .collect();
    found.sort_by(f64::total_cmp);
    found.dedup();
    found
}

fn is_comparison(token: &str) -> bool {
    matches!(token, "<" | "<=" | ">" | ">=" | "==")
}

/// The bands a player can choose between for this power, nearest first — empty when its reach
/// already settles every distance gate it has (Attune), or when it has none.
///
/// The last band runs from the last threshold to the reach, or past it when the threshold sits
/// at or beyond the reach: a leap's furthest band is what range enhancement buys.
pub fn bands(power: &Power) -> Vec<DistanceBand> {
    let reach = reach(power);
    let cuts = thresholds(power);
    if !cuts.iter().any(|cut| *cut > 0.0 && *cut < reach) {
        return Vec::new();
    }
    let inside: Vec<f64> = cuts.into_iter().filter(|cut| *cut > 0.0).collect();
    let mut bands = Vec::new();
    let mut lo = 0.0;
    for cut in &inside {
        bands.push(DistanceBand { lo, hi: *cut });
        lo = *cut;
    }
    let hi = if reach > lo { reach } else { lo * 2.0 };
    bands.push(DistanceBand { lo, hi });
    bands
}

/// [`bands`], offered only where picking one would settle something in `context` — the per-power
/// gate context with the reach standing in. A choice that would move nothing is not offered: a
/// control that moves nothing reads as a broken one.
///
/// A band settles something when, with the distance inside it, a gate the reach left
/// Indeterminate PASSES — a row or a grant appears — or a distance-scaled magnitude sits on a
/// row whose own gate holds. A gate a band only fails moves nothing: an undecided row is already
/// out of the totals, and Telekinesis's past-60 band fails its under-40 gate that way.
pub fn live_bands(power: &Power, context: &SourceContext, dataset: DatasetId) -> Vec<DistanceBand> {
    let candidates = bands(power);
    if candidates.is_empty() {
        return candidates;
    }
    let names_distance = |tokens: &[Box<str>]| tokens.iter().any(|token| &**token == "distance");
    let edges = grant_edges(power).unwrap_or_default();
    let gates: Vec<&[Box<str>]> = power
        .atoms
        .iter()
        .filter(|atom| moves_a_row(power, atom, dataset))
        .filter_map(|atom| atom.requires_expression.as_deref())
        .chain(edges.iter().filter_map(|edge| edge.condition.as_deref()))
        .filter(|gate| names_distance(gate))
        .collect();
    let scaled_gates: Vec<Option<&[Box<str>]>> = power
        .atoms
        .iter()
        .filter(|atom| {
            atom.magnitude_expression
                .as_deref()
                .is_some_and(names_distance)
        })
        .filter(|atom| moves_a_row(power, atom, dataset))
        .map(|atom| atom.requires_expression.as_deref())
        .collect();
    let at = |distance: f64| SourceContext {
        distance: Some((distance, distance)),
        ..context.clone()
    };
    let settles = |band: &DistanceBand| {
        let picked = at(band.midpoint());
        gates.iter().any(|gate| {
            eval_bool(gate, context).is_err() && matches!(eval_bool(gate, &picked), Ok(true))
        }) || scaled_gates
            .iter()
            .any(|gate| gate.is_none_or(|gate| matches!(eval_bool(gate, &picked), Ok(true))))
    };
    if candidates.iter().any(settles) {
        candidates
    } else {
        Vec::new()
    }
}

/// Would this row, let in, change what the power shows? A damage row is valued on its own gate
/// ([`crate::damage`]); any other row only through [`with_settled_gates`], and then only if the
/// display routes it somewhere. Telekinesis's under-40 and past-60 gates hold back its repel,
/// which also waits on an event, and an Intangible the panel has no row for — so a band there
/// would move nothing.
fn moves_a_row(power: &Power, atom: &AtomicEffect, dataset: DatasetId) -> bool {
    if atom.effect_type == Some(EffectType::Damage) {
        return true;
    }
    if atom.gated == Some(true) && atom.required_events.is_some() {
        return false;
    }
    let mut alone = power.clone();
    alone.atoms = vec![AtomicEffect {
        gated: None,
        ..atom.clone()
    }];
    !window_slots(&alone, dataset).keys().is_empty()
}

/// The span a distance-scaled damage program is valued over: the chosen band whole (the last
/// band open-ended, since range enhancement reaches past it), else any distance at all.
pub fn span(power: &Power, chosen: Option<f64>) -> Option<(f64, f64)> {
    let chosen = chosen?;
    let bands = bands(power);
    let last = bands.len().checked_sub(1)?;
    let index = bands.iter().position(|band| band.holds(chosen))?;
    let band = bands[index];
    Some((
        band.lo,
        if index == last {
            f64::INFINITY
        } else {
            band.hi
        },
    ))
}

/// The `distance` bounds this power's gates are answered against: the player's pick, else 0 to
/// the reach.
pub fn bounds(power: &Power, chosen: Option<f64>) -> (f64, f64) {
    match chosen {
        Some(distance) => (distance, distance),
        None => (0.0, reach(power)),
    }
}

/// `power` with the rows its distance gate held back let in, where `context`'s distance settles
/// that gate as passed — Attune's −Resistance under its 20 ft reach.
///
/// The converter marks a row `gated` when its gate is undecided without a fight to read, and
/// every display row skips a gated one. A gate that names `distance` was undecided only for want
/// of the answer this module now supplies, so once the whole gate evaluates true the row is the
/// power's own again. Only those: a row held back by a mode, a meter or a rank is someone else's
/// question, answered (or not) where it already is. So is one waiting on an event: Telekinesis
/// repels only a target its own hold has `Immobilized`, which no gate here can vouch for.
pub fn with_settled_gates<'a>(power: &'a Power, context: &SourceContext) -> Cow<'a, Power> {
    let settles = |atom: &AtomicEffect| {
        atom.gated == Some(true)
            && atom.required_events.is_none()
            && atom.requires_expression.as_deref().is_some_and(|gate| {
                gate.iter().any(|token| &**token == "distance")
                    && matches!(eval_bool(gate, context), Ok(true))
            })
    };
    if !power.atoms.iter().any(settles) {
        return Cow::Borrowed(power);
    }
    let mut owned = power.clone();
    for atom in owned.atoms.iter_mut().filter(|atom| settles(atom)) {
        atom.gated = None;
    }
    Cow::Owned(owned)
}
