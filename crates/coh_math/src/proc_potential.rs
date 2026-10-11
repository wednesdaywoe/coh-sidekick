//! Proc potential: what a power could host if slotted for procs, before any slotting exists (the
//! beta's `proc-potential.ts`).
//!
//! A power is a good proc vehicle by a coincidence of three things it owns outright: which set
//! categories it accepts (the proc inventory), its recharge (a PPM chance rises with the window,
//! up to the 90% ceiling), and its geometry (the AoE denominator taxes a wide area, unless
//! `procsOnlyOnMainTarget` exempts it).
//!
//! No single "goodness" score. Damage is the commonest goal, but -Res, knockdown, stun and -ToHit
//! procs are equally real ones, and a weighting would be the app choosing a playstyle. What comes
//! back is the composition: which effect categories are on offer and how many of each reach the
//! ceiling. [`ProcPotential::tier`] is the one ranking, and it counts rather than weighs.
//!
//! Everything is BASE potential: base recharge, no enhancement, no build. Each chance comes off
//! the same helpers [`crate::procs`] scores a slotted piece with, so the badge cannot state a
//! chance the slotted-proc rows would contradict once the piece goes in with no recharge.

use crate::procs::{
    calculate_auto_toggle_proc_chance, calculate_proc_chance, power_type_lower,
    ppm_area_denominator, proc_area, proc_fires_in, proc_patch_duration, proc_roll_schedule,
    roll_area, stat_or_default, ProcArea,
};
use coh_data::{Power, PowerDatabase, ProcData, ProcType};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The PPM formula's ceiling. A proc at this chance cannot be improved by recharge.
pub const PROC_CHANCE_CAP: f64 = 0.9;

/// One PPM proc a power could slot, with its chance of one roll at base recharge.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcPotentialEntry {
    pub set_name: String,
    pub io_name: String,
    pub ppm: f64,
    pub chance: f64,
    pub at_cap: bool,
    /// Effect category of the proc's headline effect (Damage, Control, …).
    pub category: String,
    /// Sub-type within the category (Hold, Knockdown, …), when the data names one.
    pub effect_type: Option<String>,
}

/// One effect category and how it fares in this power.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcCompositionRow {
    pub category: String,
    /// Distinct sub-types present, sorted.
    pub effect_types: Vec<String>,
    pub total: usize,
    pub at_cap: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcPotential {
    /// Highest chance first.
    pub entries: Vec<ProcPotentialEntry>,
    /// Categories with the most capped procs first. The headline output.
    pub composition: Vec<ProcCompositionRow>,
    pub at_cap: usize,
    /// Very-rare sets contributing procs.
    pub purple_sets: Vec<String>,
    /// Always-on globals (LotG, Steadfast, …) on offer. Not PPM, so counted apart.
    pub global_count: usize,
    /// The export's `maxSlots`. `None` is the wire omitting it, which earns no badge: there is no
    /// slot count to compare the capped procs against.
    pub max_slots: Option<u8>,
    /// Window one roll is scored against: base recharge, or the piece's period for an auto,
    /// toggle or patch power.
    pub window: f64,
    pub cast_time: f64,
    /// Independent rolls per activation; above 1 only on a summoned patch.
    pub rolls: f64,
    /// The AoE denominator of the power's own roll; 1.0 means no penalty.
    pub area_factor: f64,
    pub main_target_only: bool,
    /// The roll happens on a summoned patch, not the cast.
    pub from_patch: bool,
    /// `ProcAllowed kNone` with executed children rolling in its place, so `window`,
    /// `area_factor` describe the shell rather than any roll.
    pub rolls_in_executed_children: bool,
}

/// How strong a proc vehicle a power is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcTier {
    Unremarkable,
    /// Every slot could hold a proc running at the ceiling.
    ProcBomb,
    /// Twice the slots' worth of capped procs, across at least three effect categories.
    Exceptional,
}

impl ProcTier {
    pub fn label(self) -> &'static str {
        match self {
            ProcTier::Unremarkable => "",
            ProcTier::ProcBomb => "Proc bomb",
            ProcTier::Exceptional => "Exceptional proc bomb",
        }
    }
}

impl ProcPotential {
    /// `at_cap >= max_slots` states the proc-bomb proposition as a fact about the power: every
    /// slot could hold a proc at the ceiling. Pool size is the wrong headline — 14 procs at 22%
    /// (Shadow Punch) is not a proc bomb, 12 at 90% (Dark Consumption) plainly is.
    ///
    /// Patch powers land at `Unremarkable` on purpose: their rolls are scored on the piece's own
    /// period, so none approaches the ceiling. Several modest rolls is not "many procs at once".
    pub fn tier(&self) -> ProcTier {
        let Some(slots) = self.max_slots else {
            return ProcTier::Unremarkable;
        };
        let slots = usize::from(slots.max(1));
        if self.at_cap < slots {
            return ProcTier::Unremarkable;
        }
        let capped_categories = self.composition.iter().filter(|r| r.at_cap > 0).count();
        if self.at_cap >= slots * 2 && capped_categories >= 3 {
            ProcTier::Exceptional
        } else {
            ProcTier::ProcBomb
        }
    }
}

/// A proc's headline effect is its first; a secondary riding along (Gaussian's +ToHit beside its
/// +Damage) is real but not what it is slotted for.
///
/// The data files the Build Up procs under "Damage" beside the procs that DEAL damage, with an
/// "All" sub-type. Duration separates them: a damage proc resolves instantly, a timed buff
/// carries one.
fn classify(data: &ProcData) -> (String, Option<String>) {
    let Some(primary) = data.effects.first() else {
        return ("Other".to_string(), None);
    };
    if primary.category == "Damage" && primary.duration.is_some() {
        return ("Damage Buff".to_string(), None);
    }
    (primary.category.clone(), primary.effect_type.clone())
}

/// What this power could host if slotted for procs, at base recharge. `None` for a power that
/// accepts no IO set, or a dataset with no set catalog.
///
/// A `ProcAllowed kNone` power with no executed children still returns a result, with no PPM
/// entries: its globals are real. The pool is enumerated from the proc data and joined to the
/// power's slottable sets by set name, the proc data's own statement of which set a proc is in.
pub fn proc_potential(def: &Power, db: &PowerDatabase) -> Option<ProcPotential> {
    let categories = def.allowed_set_categories.as_deref().unwrap_or(&[]);
    if categories.is_empty() {
        return None;
    }
    let sets: BTreeMap<&str, &coh_data::IoSet> = db
        .io_sets
        .as_ref()?
        .sets_for_power(categories)
        .into_iter()
        .map(|(_, set)| (set.name.as_str(), set))
        .collect();

    let ptype = power_type_lower(def);
    let auto_or_toggle = ptype == "auto" || ptype == "toggle";
    let base_recharge = stat_or_default(def, "recharge", 4.0);
    let cast_time = stat_or_default(def, "castTime", 1.0);
    let own_area = proc_area(def, db);
    let from_patch = proc_patch_duration(def, db).is_some();
    let rolls_in_executed_children = def.extra.get("procsAllowed").and_then(Value::as_bool)
        == Some(false)
        && def
            .extra
            .get("procRollSites")
            .and_then(Value::as_array)
            .is_some_and(|sites| !sites.is_empty());

    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut purple = BTreeSet::new();
    let mut global_count = 0;
    // Context for the explanation, from the first scored roll; the power-level fallback below.
    let mut shown: Option<(f64, f64, f64)> = None;

    for data in db.procs.iter() {
        let Some(set) = sets.get(data.set_name.as_str()) else {
            continue;
        };
        // A proc can appear under both a bare and a set-prefixed key.
        if !seen.insert((data.set_name.as_str(), data.io_name.as_str())) {
            continue;
        }
        if data.proc_type == ProcType::Global {
            global_count += 1;
            continue;
        }
        let (ProcType::Proc, Some(ppm)) = (data.proc_type, data.ppm) else {
            continue;
        };
        // Fires nowhere (kNone with no child to take it), or a routing the one-roll model can't
        // express: either way there is no chance to state.
        let Ok(Some(site)) = proc_fires_in(def, data, &data.set_name, &data.io_name) else {
            continue;
        };
        let chance = if auto_or_toggle {
            let Some(period) = data.activate_period else {
                continue;
            };
            shown.get_or_insert((period, 0.0, 1.0));
            calculate_auto_toggle_proc_chance(ppm, period, ProcArea::single_target())
        } else {
            let area = roll_area(site.as_ref(), own_area);
            let Some(schedule) =
                proc_roll_schedule(def, db, base_recharge, cast_time, data.activate_period)
            else {
                continue;
            };
            shown.get_or_insert((schedule.window, schedule.cast_time, schedule.rolls));
            calculate_proc_chance(ppm, schedule.window, schedule.cast_time, area)
        };
        let (category, effect_type) = classify(data);
        if set.category == "purple" {
            purple.insert(set.name.clone());
        }
        entries.push(ProcPotentialEntry {
            set_name: data.set_name.clone(),
            io_name: data.io_name.clone(),
            ppm,
            chance,
            at_cap: chance >= PROC_CHANCE_CAP - 1e-9,
            category,
            effect_type,
        });
    }

    entries.sort_by(|a, b| {
        b.chance
            .total_cmp(&a.chance)
            .then_with(|| a.set_name.cmp(&b.set_name))
    });
    let (window, cast_time, rolls) = shown.unwrap_or((base_recharge, cast_time, 1.0));
    Some(ProcPotential {
        composition: composition(&entries),
        at_cap: entries.iter().filter(|e| e.at_cap).count(),
        entries,
        purple_sets: purple.into_iter().collect(),
        global_count,
        max_slots: def.max_slots(),
        window,
        cast_time,
        rolls,
        area_factor: match auto_or_toggle {
            true => 1.0,
            false => ppm_area_denominator(own_area),
        },
        main_target_only: own_area.only_main_target,
        from_patch,
        rolls_in_executed_children,
    })
}

fn composition(entries: &[ProcPotentialEntry]) -> Vec<ProcCompositionRow> {
    let mut by_category: BTreeMap<&str, ProcCompositionRow> = BTreeMap::new();
    for e in entries {
        let row = by_category
            .entry(e.category.as_str())
            .or_insert_with(|| ProcCompositionRow {
                category: e.category.clone(),
                effect_types: Vec::new(),
                total: 0,
                at_cap: 0,
            });
        row.total += 1;
        if e.at_cap {
            row.at_cap += 1;
        }
        if let Some(t) = &e.effect_type {
            if !row.effect_types.contains(t) {
                row.effect_types.push(t.clone());
            }
        }
    }
    let mut rows: Vec<ProcCompositionRow> = by_category.into_values().collect();
    for row in &mut rows {
        row.effect_types.sort();
    }
    // At-cap first: a category where everything fires is the reason to slot here.
    rows.sort_by(|a, b| {
        b.at_cap
            .cmp(&a.at_cap)
            .then(b.total.cmp(&a.total))
            .then_with(|| a.category.cmp(&b.category))
    });
    rows
}
