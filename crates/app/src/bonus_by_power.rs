//! The set-bonus finder's By power view: for the bonuses a player is chasing, which of the
//! build's own powers could carry a set that grants them, and what each would cost in slots.
//!
//! The By set view answers "which sets grant this"; this answers the question asked while
//! slotting, "where in my build can I fit another one", without opening power after power.
//! Pure over the build, the catalog and the engine's Rule-of-5 tracking, so the ordering and the
//! costs can be asked questions.

use crate::panels::stat_registry::{self, StatDef};
use coh_data::{CharacterState, EnhancementKind, IoSet, IoSetCatalog, PowerDatabase};
use coh_math::set_bonuses::{self, SetBonusStatTracking};

/// A power's slot ceiling when its def states none — the game's six.
const DEFAULT_MAX_SLOTS: usize = 6;

/// One chosen bonus as one set grants it: the lowest tier that carries it.
#[derive(Clone, PartialEq, Debug)]
pub struct Grant {
    pub row: &'static StatDef,
    pub pieces: u8,
    pub value: f64,
    /// Copies of this exact (stat, value) bonus the build already fires, accepted and refused.
    pub held: u32,
    /// Another copy would be past the Rule of 5 and grant nothing.
    pub spent: bool,
}

/// What slotting the set into the power would take.
#[derive(Clone, PartialEq, Debug)]
pub enum Cost {
    /// The power already holds enough pieces of this set.
    Slotted,
    /// The power's empty slots (plus pieces of this set it already holds) are enough.
    Fits,
    /// Enough slots, but some hold other enhancements, named here.
    Replaces(Vec<String>),
    /// More slots than the power has; any enhancements in the way are named too.
    NeedsSlots { more: usize, replaces: Vec<String> },
}

impl Cost {
    /// Cheapest new copy first. A set already slotted comes last: it is not another copy, so it
    /// answers "where else" only when nothing else does.
    fn rank(&self) -> u8 {
        match self {
            Cost::Fits => 0,
            Cost::Replaces(_) => 1,
            Cost::NeedsSlots { .. } => 2,
            Cost::Slotted => 3,
        }
    }
}

/// One power that could take one set granting at least one chosen bonus.
#[derive(Clone, PartialEq, Debug)]
pub struct Route {
    pub power_name: String,
    pub power_level: u8,
    pub slots: usize,
    pub set_id: String,
    pub set_name: String,
    pub set_category: String,
    /// The chosen bonuses this set grants, in the order they were chosen.
    pub grants: Vec<Grant>,
    /// Pieces needed for every grant above.
    pub pieces: u8,
    pub cost: Cost,
}

impl Route {
    /// Every grant here would be past the Rule of 5 — and the power doesn't already count it.
    pub fn all_spent(&self) -> bool {
        self.cost != Cost::Slotted && self.grants.iter().all(|grant| grant.spent)
    }
}

/// Every (power, set) pair in the build where the set grants one of `chosen` (stat row ids).
///
/// New copies before sets already slotted, then by how many chosen bonuses the set grants (most
/// first), then whether any of them would still count under the Rule of 5, then by cost (fits,
/// replaces, needs slots), then fewest pieces, then the build's own power order and set name.
pub fn routes(
    build: &CharacterState,
    database: &PowerDatabase,
    catalog: &IoSetCatalog,
    chosen: &[String],
    tracking: &[SetBonusStatTracking],
) -> Vec<Route> {
    let rows: Vec<&'static StatDef> = chosen
        .iter()
        .filter_map(|id| stat_registry::by_id(id))
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(usize, Route)> = Vec::new();
    for (order, pick) in build.all_selected().enumerate() {
        let Some(power) = database.resolve_power(&pick.powerset, &pick.internal_name) else {
            continue;
        };
        let Some(categories) = power.allowed_set_categories.as_deref() else {
            continue;
        };
        if !power.accepts_enhancements() || power.max_slots() == Some(0) {
            continue;
        }
        let max_slots = power.max_slots().map_or(DEFAULT_MAX_SLOTS, usize::from);
        for (set_id, set) in catalog.sets_for_power(categories) {
            let grants = grants(set, &rows, tracking);
            let Some(pieces) = grants.iter().map(|grant| grant.pieces).max() else {
                continue;
            };
            if usize::from(pieces) > max_slots {
                continue;
            }
            found.push((
                order,
                Route {
                    power_name: power.name.clone(),
                    power_level: pick.level,
                    slots: pick.slots.len(),
                    set_id: set_id.to_string(),
                    set_name: set.name.clone(),
                    set_category: set.category.clone(),
                    grants,
                    pieces,
                    cost: cost(&pick.slots, set_id, pieces),
                },
            ));
        }
    }
    found.sort_by(|(a_order, a), (b_order, b)| {
        let slotted = |route: &Route| route.cost == Cost::Slotted;
        slotted(a)
            .cmp(&slotted(b))
            .then(b.grants.len().cmp(&a.grants.len()))
            .then(a.all_spent().cmp(&b.all_spent()))
            .then(a.cost.rank().cmp(&b.cost.rank()))
            .then(a.pieces.cmp(&b.pieces))
            .then(a_order.cmp(b_order))
            .then(a.set_name.cmp(&b.set_name))
    });
    found.into_iter().map(|(_, route)| route).collect()
}

/// Each power's first route only — its best, since [`routes`] is already in order — with how
/// many other sets that power could take for the same bonuses.
pub fn best_per_power(routes: Vec<Route>) -> Vec<(Route, usize)> {
    let mut best: Vec<(Route, usize)> = Vec::new();
    for route in routes {
        match best.iter_mut().find(|(kept, _)| {
            kept.power_name == route.power_name && kept.power_level == route.power_level
        }) {
            Some((_, others)) => *others += 1,
            None => best.push((route, 0)),
        }
    }
    best
}

/// The lowest tier of `set` granting each row, with its Rule-of-5 standing.
fn grants(set: &IoSet, rows: &[&'static StatDef], tracking: &[SetBonusStatTracking]) -> Vec<Grant> {
    rows.iter()
        .filter_map(|row| {
            set.bonuses
                .iter()
                .filter_map(|tier| {
                    tier.effects
                        .iter()
                        .find(|effect| {
                            crate::tracked_stats::rows_for_effect(effect).contains(&row.id)
                        })
                        .map(|effect| (tier.pieces, effect))
                })
                .min_by_key(|(pieces, _)| *pieces)
                .map(|(pieces, effect)| {
                    let held = match set_bonuses::effect_stat_key(&effect.stat) {
                        set_bonuses::EffectStatKey::Tracked(key) => {
                            set_bonuses::bonus_tracking_lookup(tracking, key, effect.value).0
                        }
                        _ => 0,
                    };
                    Grant {
                        row,
                        pieces,
                        value: effect.value,
                        held,
                        spent: held >= 5,
                    }
                })
        })
        .collect()
}

/// What `pieces` of the set would take in a power whose slots are `slots` now.
fn cost(slots: &[Option<coh_data::Enhancement>], set_id: &str, pieces: u8) -> Cost {
    let pieces = usize::from(pieces);
    let in_set = |kind: &EnhancementKind| matches!(kind, EnhancementKind::IoSet { set_id: id, .. } if id == set_id);
    let held = slots
        .iter()
        .flatten()
        .filter(|piece| in_set(&piece.kind))
        .count();
    if held >= pieces {
        return Cost::Slotted;
    }
    let empty = slots.iter().filter(|slot| slot.is_none()).count();
    let needed = pieces - held;
    if needed <= empty {
        return Cost::Fits;
    }
    // The slots this set would take over: everything not already empty or already this set,
    // up to the number still short after the empty ones.
    let others: Vec<&coh_data::Enhancement> = slots
        .iter()
        .flatten()
        .filter(|piece| !in_set(&piece.kind))
        .collect();
    let displaced = (needed - empty).min(others.len());
    let mut replaces: Vec<String> = others
        .iter()
        .take(displaced)
        .map(|piece| match &piece.kind {
            EnhancementKind::IoSet { set_name, .. } => set_name.clone(),
            _ => crate::naming::enhancement_name(piece).to_string(),
        })
        .collect();
    replaces.dedup();
    if pieces <= slots.len() {
        Cost::Replaces(replaces)
    } else {
        Cost::NeedsSlots {
            more: pieces - slots.len(),
            replaces,
        }
    }
}

/// The build's copies of each value of a chosen bonus: `(value, accepted, capped)`, highest value
/// first. A stat tracks each value separately, so "+3%" and "+1.5%" have five apiece.
pub fn held_copies(row: &StatDef, tracking: &[SetBonusStatTracking]) -> Vec<(f64, usize, bool)> {
    let mut copies: Vec<(f64, usize, bool)> = tracking
        .iter()
        .filter(|stat| {
            stat.breakdown_keys
                .iter()
                .any(|key| row.breakdown_keys.contains(&key.as_str()))
        })
        .flat_map(|stat| stat.buckets.values())
        .map(|bucket| (bucket.value, bucket.sources.len(), bucket.capped))
        .collect();
    copies.sort_by(|a, b| b.0.total_cmp(&a.0));
    copies
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::DatasetId;
    use std::path::PathBuf;

    fn database() -> PowerDatabase {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contract/homecoming/bundle.json.gz");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
    }

    fn charnel(database: &PowerDatabase) -> CharacterState {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/buildsave/homecoming/charnel.txt");
        let text = std::fs::read_to_string(&path).expect("fixture reads");
        crate::build_file::decode_import(&text, database, DatasetId::Homecoming)
            .expect("fixture imports")
            .build
    }

    fn chosen() -> Vec<String> {
        vec!["res_energy".to_string(), "res_negative".to_string()]
    }

    /// Every route can really be slotted there, grants what it says, and costs what its slots say.
    #[test]
    fn every_route_is_a_set_the_power_takes_and_costs_what_its_slots_say() {
        let database = database();
        let catalog = database.io_sets.as_ref().expect("catalog");
        let build = charnel(&database);
        let totals = coh_math::recalculate(&build, &database);
        let found = routes(
            &build,
            &database,
            catalog,
            &chosen(),
            &totals.set_bonus_tracking,
        );
        assert!(found.len() > 3, "{}", found.len());
        for route in &found {
            let set = &catalog.sets[&route.set_id];
            let pick = build
                .all_selected()
                .find(|pick| {
                    database
                        .resolve_power(&pick.powerset, &pick.internal_name)
                        .is_some_and(|power| power.name == route.power_name)
                })
                .expect("route names a picked power");
            let power = database
                .resolve_power(&pick.powerset, &pick.internal_name)
                .expect("resolves");
            assert!(power
                .allowed_set_categories
                .as_ref()
                .is_some_and(|c| c.contains(&set.set_type)));
            assert!(!route.grants.is_empty());
            for grant in &route.grants {
                assert!(crate::tracked_stats::has_tracked_bonus(
                    set,
                    &[grant.row.id.to_string()]
                ));
            }
            let held = pick
                .slots
                .iter()
                .flatten()
                .filter(|p| matches!(&p.kind, EnhancementKind::IoSet { set_id, .. } if *set_id == route.set_id))
                .count();
            let empty = pick.slots.iter().filter(|s| s.is_none()).count();
            let pieces = usize::from(route.pieces);
            let fits = match route.cost {
                Cost::Slotted => held >= pieces,
                Cost::Fits => held < pieces && pieces - held <= empty,
                Cost::Replaces(_) => pieces - held > empty && pieces <= pick.slots.len(),
                Cost::NeedsSlots { more, .. } => more == pieces - pick.slots.len(),
            };
            assert!(fits, "{route:?} held {held} empty {empty}");
        }
    }

    /// Among new copies, sets granting both chosen bonuses come before sets granting one. Energy
    /// and Negative resistance always come as a pair on Homecoming, so this asks for two that
    /// don't.
    #[test]
    fn sets_granting_every_chosen_bonus_sort_first() {
        let database = database();
        let catalog = database.io_sets.as_ref().expect("catalog");
        let build = charnel(&database);
        let chosen = vec!["res_energy".to_string(), "recharge".to_string()];
        let found = routes(&build, &database, catalog, &chosen, &[]);
        let counts: Vec<usize> = found
            .iter()
            .filter(|route| route.cost != Cost::Slotted)
            .map(|route| route.grants.len())
            .collect();
        assert!(counts.contains(&2) && counts.contains(&1), "{counts:?}");
        assert!(counts.windows(2).all(|w| w[0] >= w[1]), "{counts:?}");
    }

    /// A power's best row is a new copy when one exists, not the set it already holds.
    #[test]
    fn a_new_copy_outranks_the_set_already_slotted() {
        let database = database();
        let catalog = database.io_sets.as_ref().expect("catalog");
        let build = charnel(&database);
        let found = routes(&build, &database, catalog, &chosen(), &[]);
        for (route, others) in best_per_power(found.clone()) {
            if route.cost == Cost::Slotted {
                assert_eq!(others, 0, "{} had a new option", route.power_name);
            }
        }
    }

    /// One row per power, each its power's first route, and nothing lost from the count.
    #[test]
    fn best_per_power_keeps_each_powers_first_route() {
        let database = database();
        let catalog = database.io_sets.as_ref().expect("catalog");
        let build = charnel(&database);
        let found = routes(&build, &database, catalog, &chosen(), &[]);
        let best = best_per_power(found.clone());
        assert_eq!(
            best.iter().map(|(_, others)| others + 1).sum::<usize>(),
            found.len()
        );
        for (route, _) in &best {
            let first = found
                .iter()
                .find(|r| r.power_name == route.power_name && r.power_level == route.power_level);
            assert_eq!(first, Some(route));
        }
        let mut names: Vec<&str> = best.iter().map(|(r, _)| r.power_name.as_str()).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "one row per power");
    }

    /// The build's own Unbreakable Guard is reported as already slotted, not as a place to add one.
    #[test]
    fn a_set_the_power_already_holds_is_slotted() {
        let database = database();
        let catalog = database.io_sets.as_ref().expect("catalog");
        let build = charnel(&database);
        let found = routes(&build, &database, catalog, &chosen(), &[]);
        assert!(found
            .iter()
            .any(|route| route.set_name == "Unbreakable Guard" && route.cost == Cost::Slotted));
    }
}
