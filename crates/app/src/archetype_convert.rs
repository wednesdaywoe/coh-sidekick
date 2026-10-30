//! Copying a build to another archetype: the power sets the two archetypes share come across with
//! their picks and slotting, and everything that could not is listed for the player to read
//! before the copy is made.
//!
//! Sets are matched by name across the two archetypes' rosters, because each archetype carries
//! its own copy of a set (`blaster/fire-blast`, `defender/fire-blast`) with its own unlock levels.
//! Powers inside a set are matched by internal name, then by display name; a power the new copy
//! of the set lacks is reported, never guessed at. Pure over the build and the database, so the
//! report can be asked questions.

use crate::panels::powers::{set_unlock_floor, SetOrigin};
use coh_data::{
    Archetype, CharacterState, Enhancement, EnhancementCatalog, EnhancementKind, IoSetCatalog,
    Power, PowerDatabase, Powerset, PowersetSelection, SelectedPower,
};

/// Which of the archetype's two power sets a set sits in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Primary,
    Secondary,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Primary => "primary",
            Role::Secondary => "secondary",
        }
    }
}

/// One of the old build's power sets, and where it went.
#[derive(Clone, PartialEq, Debug)]
pub struct SetMove {
    pub name: String,
    pub from: Role,
    /// `None`: the new archetype has no set of this name.
    pub to: Option<Role>,
}

/// A pick that came across at a different level.
#[derive(Clone, PartialEq, Debug)]
pub struct Moved {
    pub power: String,
    pub from: u8,
    pub to: u8,
}

/// Why a pick did not come across.
#[derive(Clone, PartialEq, Debug)]
pub enum DropWhy {
    /// Its set is not one the new archetype has.
    SetNotShared,
    /// The new archetype's copy of the set has no power by this name.
    NoMatch,
    /// The power's own prerequisite refuses the new archetype (an epic pool, mostly).
    Refused,
    /// No power pick is left at or above the level the power unlocks at.
    NoPickLeft,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Dropped {
    pub power: String,
    pub set: String,
    pub level: u8,
    pub why: DropWhy,
}

/// An enhancement the carried power can no longer hold, so its slot came across empty.
#[derive(Clone, PartialEq, Debug)]
pub struct DroppedPiece {
    pub power: String,
    /// The IO set the piece belongs to, when it is a set piece.
    pub set: Option<String>,
    pub piece: String,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Conversion {
    pub build: CharacterState,
    pub sets: Vec<SetMove>,
    /// The new archetype's sets that nothing came across into, left for the player to pick.
    pub empty: Vec<Role>,
    pub moved: Vec<Moved>,
    pub dropped: Vec<Dropped>,
    pub pieces: Vec<DroppedPiece>,
    /// The old archetype's own inherent, when the new archetype has a different one.
    pub inherent: Option<String>,
}

/// Copy `build` to the archetype `target`. `None` when the target is unknown or is the build's
/// own archetype. The original is not touched.
pub fn convert_build(
    build: &CharacterState,
    database: &PowerDatabase,
    target: &str,
) -> Option<Conversion> {
    let catalog = database.archetypes().ok()?;
    let archetype = catalog.get(target)?;
    if build.archetype.id.as_deref() == Some(target) {
        return None;
    }
    let old_inherent = crate::inherents::declared_inherent_name(build, database);

    let mut copy = build.clone();
    copy.archetype = coh_data::ArchetypeSelection {
        id: Some(archetype.id.clone()),
        name: archetype.name.clone(),
    };
    // A new build: saving it must not overwrite the original's cloud row.
    copy.library_id = None;
    copy.primary = PowersetSelection::default();
    copy.secondary = PowersetSelection::default();

    let mut sets = Vec::new();
    let mut dropped = Vec::new();
    // (old set id, new set id) for every set that came across, to re-address per-power records.
    let mut renamed: Vec<(String, String, Role, Role)> = Vec::new();
    for (from, held) in [
        (Role::Primary, &build.primary),
        (Role::Secondary, &build.secondary),
    ] {
        let Some(old_id) = held.id.as_deref() else {
            continue;
        };
        let name = database
            .find_powerset(old_id)
            .map(|set| set.name.clone())
            .unwrap_or_else(|| held.name.clone());
        let found = matching_set(database, archetype, &name)
            .filter(|(role, _)| slot(&mut copy, *role).id.is_none());
        sets.push(SetMove {
            name: name.clone(),
            from,
            to: found.map(|(role, _)| role),
        });
        let Some((to, set)) = found else {
            dropped.extend(held.powers.iter().map(|pick| Dropped {
                power: display_name(database, pick),
                set: name.clone(),
                level: pick.level,
                why: DropWhy::SetNotShared,
            }));
            continue;
        };
        let mut powers = Vec::new();
        for pick in &held.powers {
            let old_name = database
                .resolve_power(&pick.powerset, &pick.internal_name)
                .map(|power| power.name.clone());
            let matched = set
                .powers
                .iter()
                .find(|power| power.ident() == pick.internal_name)
                .or_else(|| {
                    old_name
                        .as_ref()
                        .and_then(|name| set.powers.iter().find(|power| &power.name == name))
                });
            match matched {
                Some(power) => powers.push(SelectedPower {
                    internal_name: power.ident().to_string(),
                    powerset: set.id.clone(),
                    ..pick.clone()
                }),
                None => dropped.push(Dropped {
                    power: old_name.unwrap_or_else(|| pick.internal_name.clone()),
                    set: name.clone(),
                    level: pick.level,
                    why: DropWhy::NoMatch,
                }),
            }
        }
        *slot(&mut copy, to) = PowersetSelection {
            id: Some(set.id.clone()),
            name: set.name.clone(),
            powers,
        };
        renamed.push((old_id.to_string(), set.id.clone(), from, to));
    }
    let empty = [Role::Primary, Role::Secondary]
        .into_iter()
        .filter(|role| slot(&mut copy, *role).id.is_none())
        .collect();

    dropped.extend(drop_refused(&mut copy, build, database, target));
    let (moved, unplaced) = place_picks(&mut copy, database);
    dropped.extend(unplaced);
    let pieces = drop_unfit_pieces(&mut copy, database);
    readdress(&mut copy, build, &renamed);

    crate::inherents::sync(&mut copy, database);
    crate::granted_powers::sync(&mut copy, database);
    let inherent = old_inherent.filter(|old| {
        crate::inherents::declared_inherent_name(&copy, database).as_ref() != Some(old)
    });

    Some(Conversion {
        build: copy,
        sets,
        empty,
        moved,
        dropped,
        pieces,
        inherent,
    })
}

/// The new archetype's own copy of a set, found by name, and the position it holds there.
fn matching_set<'a>(
    database: &'a PowerDatabase,
    archetype: &Archetype,
    name: &str,
) -> Option<(Role, &'a Powerset)> {
    [
        (Role::Primary, &archetype.primary_sets),
        (Role::Secondary, &archetype.secondary_sets),
    ]
    .into_iter()
    .find_map(|(role, ids)| {
        ids.iter()
            .filter_map(|id| database.find_powerset(id))
            .find(|set| set.name == name)
            .map(|set| (role, set))
    })
}

/// The build's power sets `archetype` also has, by name: what a conversion to it would keep.
pub fn shared_sets(
    build: &CharacterState,
    database: &PowerDatabase,
    archetype: &Archetype,
) -> Vec<String> {
    [&build.primary, &build.secondary]
        .into_iter()
        .filter_map(|held| held.id.as_deref())
        .filter_map(|id| database.find_powerset(id))
        .filter(|set| matching_set(database, archetype, &set.name).is_some())
        .map(|set| set.name.clone())
        .collect()
}

fn slot(build: &mut CharacterState, role: Role) -> &mut PowersetSelection {
    match role {
        Role::Primary => &mut build.primary,
        Role::Secondary => &mut build.secondary,
    }
}

fn display_name(database: &PowerDatabase, pick: &SelectedPower) -> String {
    database
        .resolve_power(&pick.powerset, &pick.internal_name)
        .map(|power| power.name.clone())
        .unwrap_or_else(|| pick.internal_name.clone())
}

/// Where a pick lives in the build, so the passes below can walk picks by index and still edit.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Bucket {
    Primary,
    Secondary,
    Pool(usize),
    Epic,
}

fn buckets(build: &CharacterState) -> Vec<(Bucket, usize)> {
    let mut all = Vec::new();
    all.extend((0..build.primary.powers.len()).map(|i| (Bucket::Primary, i)));
    all.extend((0..build.secondary.powers.len()).map(|i| (Bucket::Secondary, i)));
    for (pool, held) in build.pools.iter().enumerate() {
        all.extend((0..held.powers.len()).map(|i| (Bucket::Pool(pool), i)));
    }
    if let Some(epic) = &build.epic_pool {
        all.extend((0..epic.powers.len()).map(|i| (Bucket::Epic, i)));
    }
    all
}

fn bucket_picks(build: &CharacterState, bucket: Bucket) -> &[SelectedPower] {
    match bucket {
        Bucket::Primary => &build.primary.powers,
        Bucket::Secondary => &build.secondary.powers,
        Bucket::Pool(pool) => &build.pools[pool].powers,
        Bucket::Epic => build.epic_pool.as_ref().map_or(&[], |epic| &epic.powers),
    }
}

fn bucket_powers(build: &mut CharacterState, bucket: Bucket) -> &mut Vec<SelectedPower> {
    match bucket {
        Bucket::Primary => &mut build.primary.powers,
        Bucket::Secondary => &mut build.secondary.powers,
        Bucket::Pool(pool) => &mut build.pools[pool].powers,
        Bucket::Epic => {
            &mut build
                .epic_pool
                .as_mut()
                .expect("an epic bucket comes from a held epic pool")
                .powers
        }
    }
}

/// The set's name as the report prints it. An imported build can carry a pool with no name, so
/// the catalog's is the fallback.
fn bucket_name(database: &PowerDatabase, build: &CharacterState, bucket: Bucket) -> String {
    let pool = match bucket {
        Bucket::Primary => return build.primary.name.clone(),
        Bucket::Secondary => return build.secondary.name.clone(),
        Bucket::Pool(pool) => &build.pools[pool],
        Bucket::Epic => match &build.epic_pool {
            Some(epic) => epic,
            None => return String::new(),
        },
    };
    if !pool.name.is_empty() {
        return pool.name.clone();
    }
    database
        .pool_catalog
        .find(&pool.id)
        .map_or_else(|| pool.id.clone(), |def| def.name.clone())
}

/// Drop every pick whose own `requires` refuses the copy, until none does: an epic pool's opener
/// carries the archetype gate, and its later powers need an earlier one, so they go together.
/// Read in pick order, as the game asked it, or the later powers would count each other. Only a
/// gate the original passed can refuse: a pick already out of order there is the planner's to
/// flag, not this copy's to drop. An unreadable gate keeps its pick, as the picker keeps its row.
fn drop_refused(
    build: &mut CharacterState,
    original: &CharacterState,
    database: &PowerDatabase,
    target: &str,
) -> Vec<Dropped> {
    let passes = |state: &CharacterState, archetype: Option<&str>, pick: &SelectedPower| {
        let Some(power) = database.resolve_power(&pick.powerset, &pick.internal_name) else {
            return true;
        };
        let gate = coh_data::granted_powers::requires(power).unwrap_or_default();
        coh_data::requires_met_in_order(&gate, state, pick.level, archetype, &database.set_paths)
            != Ok(false)
    };
    let passed_before = |pick: &SelectedPower| {
        original
            .all_selected()
            .find(|old| old.internal_name == pick.internal_name && old.level == pick.level)
            .is_none_or(|old| passes(original, original.archetype.id.as_deref(), old))
    };
    let mut dropped = Vec::new();
    loop {
        let refused = buckets(build).into_iter().find(|&(bucket, index)| {
            let pick = &bucket_picks(build, bucket)[index];
            !passes(build, Some(target), pick) && passed_before(pick)
        });
        let Some((bucket, index)) = refused else {
            break;
        };
        let set = bucket_name(database, build, bucket);
        let pick = bucket_powers(build, bucket).remove(index);
        dropped.push(Dropped {
            power: display_name(database, &pick),
            set,
            level: pick.level,
            why: DropWhy::Refused,
        });
    }
    build.pools.retain(|pool| !pool.powers.is_empty());
    if build
        .epic_pool
        .as_ref()
        .is_some_and(|epic| epic.powers.is_empty())
    {
        build.epic_pool = None;
    }
    dropped
}

/// The lowest level `pick` may be taken at in `build`: its own unlock level, and for a pool or an
/// epic power the aggregate's floor too.
fn floor(database: &PowerDatabase, bucket: Bucket, pick: &SelectedPower) -> u8 {
    let own = database
        .resolve_power(&pick.powerset, &pick.internal_name)
        .map_or(1, Power::unlock_level);
    let origin = match bucket {
        Bucket::Primary | Bucket::Secondary => SetOrigin::Powerset,
        Bucket::Pool(_) => SetOrigin::Pool,
        Bucket::Epic => SetOrigin::Epic,
    };
    own.max(set_unlock_floor(database, &pick.powerset, origin))
}

/// Put every pick at a level the copy allows, moving as few as possible and each as little as
/// possible: a pick still legal where it stood stays, unless another pick needs its level more.
/// Whatever finds no pick at or above its unlock level is dropped. Level 1 is character creation,
/// so only a pick already there may sit there; a power the game hands out (level 0) holds no pick
/// and is left alone.
fn place_picks(build: &mut CharacterState, database: &PowerDatabase) -> (Vec<Moved>, Vec<Dropped>) {
    let Some(schedule) = database.leveling_schedule.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    struct Seat {
        bucket: Bucket,
        index: usize,
        old: u8,
        floor: u8,
        level: Option<u8>,
    }
    let mut seats: Vec<Seat> = buckets(build)
        .into_iter()
        .filter(|&(bucket, index)| bucket_picks(build, bucket)[index].level > 0)
        .map(|(bucket, index)| {
            let pick = &bucket_picks(build, bucket)[index];
            Seat {
                bucket,
                index,
                old: pick.level,
                floor: floor(database, bucket, pick),
                level: None,
            }
        })
        .collect();
    let picks = schedule.pick_levels();

    // One column per pick the schedule grants, then one "dropped" column per seat so every seat
    // has somewhere to go. Any move outweighs every distance, and a drop outweighs any move.
    const MOVE: i64 = 1_000;
    const DROP: i64 = 1_000_000;
    const NEVER: i64 = 1_000_000_000;
    let mut cost: Vec<Vec<i64>> = seats
        .iter()
        .map(|seat| {
            let to_pick = picks.iter().map(|&level| {
                if level < seat.floor || (level == 1 && seat.old != 1) {
                    NEVER
                } else if level == seat.old {
                    0
                } else {
                    MOVE + i64::from(level.abs_diff(seat.old))
                }
            });
            to_pick.chain(seats.iter().map(|_| DROP)).collect()
        })
        .collect();
    // A move must not put a power before the one it requires (Tough before Boxing): refuse that
    // level for that power and solve again, until no move breaks an order.
    let archetype = build.archetype.id.clone();
    loop {
        for (seat, column) in seats.iter_mut().zip(cheapest_assignment(&cost)) {
            seat.level = picks.get(column).copied();
        }
        let mut trial = build.clone();
        for seat in &seats {
            bucket_powers(&mut trial, seat.bucket)[seat.index].level = seat.level.unwrap_or(0);
        }
        let mut refused = false;
        for (row, seat) in seats.iter().enumerate() {
            let Some(level) = seat.level.filter(|&level| level != seat.old) else {
                continue;
            };
            let pick = &bucket_picks(&trial, seat.bucket)[seat.index];
            let Some(power) = database.resolve_power(&pick.powerset, &pick.internal_name) else {
                continue;
            };
            let gate = coh_data::granted_powers::requires(power).unwrap_or_default();
            let in_order = coh_data::requires_met_in_order(
                &gate,
                &trial,
                level,
                archetype.as_deref(),
                &database.set_paths,
            );
            if in_order == Ok(false) {
                for (column, &at) in picks.iter().enumerate() {
                    if at == level {
                        cost[row][column] = NEVER;
                    }
                }
                refused = true;
            }
        }
        if !refused {
            break;
        }
    }

    let mut moved = Vec::new();
    let mut dropped = Vec::new();
    let mut gone: Vec<(Bucket, usize)> = Vec::new();
    for seat in &seats {
        let set = bucket_name(database, build, seat.bucket);
        let pick = &mut bucket_powers(build, seat.bucket)[seat.index];
        match seat.level {
            Some(level) => {
                if level != seat.old {
                    moved.push(Moved {
                        power: display_name(database, pick),
                        from: seat.old,
                        to: level,
                    });
                    pick.level = level;
                }
            }
            None => {
                dropped.push(Dropped {
                    power: display_name(database, pick),
                    set,
                    level: seat.old,
                    why: DropWhy::NoPickLeft,
                });
                gone.push((seat.bucket, seat.index));
            }
        }
    }
    // Highest index first, so the earlier indices stay valid.
    gone.sort_by_key(|&(_, index)| std::cmp::Reverse(index));
    for (bucket, index) in gone {
        bucket_powers(build, bucket).remove(index);
    }
    (moved, dropped)
}

/// The column for each row that makes the total cost least (the Hungarian method, with
/// potentials). `cost` has at least as many columns as rows; each row gets a different column.
fn cheapest_assignment(cost: &[Vec<i64>]) -> Vec<usize> {
    let rows = cost.len();
    let Some(columns) = cost.first().map(Vec::len) else {
        return Vec::new();
    };
    // 1-based, with row 0 / column 0 as the solver's sentinel.
    let mut u = vec![0i64; rows + 1];
    let mut v = vec![0i64; columns + 1];
    let mut owner = vec![0usize; columns + 1];
    let mut way = vec![0usize; columns + 1];
    for row in 1..=rows {
        owner[0] = row;
        let mut column = 0;
        let mut least = vec![i64::MAX; columns + 1];
        let mut used = vec![false; columns + 1];
        loop {
            used[column] = true;
            let at = owner[column];
            let mut delta = i64::MAX;
            let mut next = 0;
            for j in 1..=columns {
                if used[j] {
                    continue;
                }
                let reduced = cost[at - 1][j - 1] - u[at] - v[j];
                if reduced < least[j] {
                    least[j] = reduced;
                    way[j] = column;
                }
                if least[j] < delta {
                    delta = least[j];
                    next = j;
                }
            }
            for j in 0..=columns {
                if used[j] {
                    u[owner[j]] += delta;
                    v[j] -= delta;
                } else {
                    least[j] -= delta;
                }
            }
            column = next;
            if owner[column] == 0 {
                break;
            }
        }
        while column != 0 {
            let previous = way[column];
            owner[column] = owner[previous];
            column = previous;
        }
    }
    let mut assigned = vec![0; rows];
    for j in 1..=columns {
        if owner[j] != 0 {
            assigned[owner[j] - 1] = j - 1;
        }
    }
    assigned
}

/// Empty every slot whose enhancement the carried power's new definition cannot hold: an
/// archetype's own sets, and any set or type the new copy of the power does not take. The slot
/// itself stays.
fn drop_unfit_pieces(build: &mut CharacterState, database: &PowerDatabase) -> Vec<DroppedPiece> {
    let mut dropped = Vec::new();
    for pick in build
        .primary
        .powers
        .iter_mut()
        .chain(build.secondary.powers.iter_mut())
    {
        let Some(power) = database.resolve_power(&pick.powerset, &pick.internal_name) else {
            continue;
        };
        for held in pick.slots.iter_mut() {
            let fits = held.as_ref().is_none_or(|piece| {
                fits(
                    piece,
                    power,
                    database.io_sets.as_ref(),
                    database.enhancements.as_ref(),
                )
            });
            if !fits {
                if let Some(piece) = held.take() {
                    let set = match &piece.kind {
                        EnhancementKind::IoSet { set_name, .. } => Some(set_name.clone()),
                        _ => None,
                    };
                    dropped.push(DroppedPiece {
                        power: power.name.clone(),
                        set,
                        piece: crate::naming::enhancement_name(&piece).to_string(),
                    });
                }
            }
        }
    }
    dropped
}

fn fits(
    piece: &Enhancement,
    power: &Power,
    sets: Option<&IoSetCatalog>,
    catalog: Option<&EnhancementCatalog>,
) -> bool {
    let allowed = power.allowed_enhancements.as_deref();
    let takes = |stat: &str| allowed.is_none_or(|allowed| allowed.iter().any(|a| a == stat));
    match &piece.kind {
        EnhancementKind::IoSet { set_id, .. } => {
            let Some(set) = sets.and_then(|sets| sets.sets.get(set_id)) else {
                return true;
            };
            power
                .allowed_set_categories
                .as_deref()
                .is_some_and(|categories| categories.iter().any(|c| c == &set.set_type))
        }
        EnhancementKind::GenericIo { stat, .. } => catalog.is_none_or(|catalog| {
            catalog
                .generic_ios_for_power(allowed)
                .contains(&stat.as_str())
        }),
        EnhancementKind::Origin { stat, .. } => takes(stat),
        EnhancementKind::Special { aspects, .. } => {
            aspects.is_empty() || aspects.iter().any(|aspect| takes(&aspect.stat))
        }
    }
}

/// Carry the per-power records keyed by set id over to the new set ids, and the slot history's
/// primary / secondary tags over to the sets' new positions; records for powers that did not come
/// across are dropped.
fn readdress(
    copy: &mut CharacterState,
    old: &CharacterState,
    renamed: &[(String, String, Role, Role)],
) {
    let present: std::collections::BTreeSet<String> =
        copy.all_selected().map(SelectedPower::address).collect();
    let renamed_address = |address: &str| -> Option<String> {
        let (set, rest) = address.split_once(':')?;
        let set = renamed
            .iter()
            .find(|(from, ..)| from == set)
            .map_or(set, |(_, to, ..)| to.as_str());
        let moved = format!("{set}:{rest}");
        let power = moved
            .rsplit_once(':')
            .map_or(moved.as_str(), |(head, _)| head);
        (present.contains(&moved) || present.contains(power)).then_some(moved)
    };
    copy.proc_overrides = old
        .proc_overrides
        .iter()
        .filter_map(|(key, value)| Some((renamed_address(key)?, value.clone())))
        .collect();
    for chain in copy.attack_chains.iter_mut() {
        chain.powers = chain
            .powers
            .iter()
            .filter_map(|address| renamed_address(address))
            .collect();
    }

    let tag = |role: Role| role.label().to_string();
    for entry in copy.slot_order.iter_mut() {
        let Some(category) = entry.category.as_deref() else {
            continue;
        };
        if let Some((.., to)) = renamed.iter().find(|(.., from, _)| tag(*from) == category) {
            entry.category = Some(tag(*to));
        }
    }
    let held = |category: Option<&str>, name: &str| {
        let mut picks: Box<dyn Iterator<Item = &SelectedPower>> = match category {
            Some("primary") => Box::new(copy.primary.powers.iter()),
            Some("secondary") => Box::new(copy.secondary.powers.iter()),
            _ => Box::new(copy.all_selected()),
        };
        picks.any(|pick| pick.internal_name == name)
    };
    let keep: Vec<bool> = copy
        .slot_order
        .iter()
        .map(|entry| held(entry.category.as_deref(), &entry.power_name))
        .collect();
    let mut keep = keep.into_iter();
    copy.slot_order.retain(|_| keep.next().unwrap_or(true));
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

    fn fixture(database: &PowerDatabase, name: &str) -> CharacterState {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/buildsave/homecoming/{name}.txt"));
        let text = std::fs::read_to_string(&path).expect("fixture reads");
        let mut build = crate::build_file::decode_import(&text, database, DatasetId::Homecoming)
            .expect("fixture imports")
            .build;
        crate::inherents::sync(&mut build, database);
        crate::granted_powers::sync(&mut build, database);
        build
    }

    /// Convert a fixture, and check the original came through untouched.
    fn convert(database: &PowerDatabase, build: &CharacterState, to: &str) -> Conversion {
        let before = build.clone();
        let conversion = convert_build(build, database, to).expect("converts");
        assert_eq!(build, &before, "the original build is unchanged");
        assert_eq!(conversion.build.archetype.id.as_deref(), Some(to));
        conversion
    }

    fn idents(set: &PowersetSelection) -> Vec<&str> {
        set.powers
            .iter()
            .map(|pick| pick.internal_name.as_str())
            .collect()
    }

    fn slotted(set: &PowersetSelection, ident: &str) -> usize {
        set.powers
            .iter()
            .find(|pick| pick.internal_name == ident)
            .map_or(0, |pick| pick.slots.len())
    }

    #[test]
    fn scrapper_to_brute_keeps_both_sets_in_place() {
        let database = database();
        let build = fixture(&database, "maidenfury");
        let conversion = convert(&database, &build, "brute");
        let copy = &conversion.build;
        assert_eq!(copy.primary.id.as_deref(), Some("brute/martial-arts"));
        assert_eq!(copy.secondary.id.as_deref(), Some("brute/shield-defense"));
        assert_eq!(idents(&copy.primary), idents(&build.primary));
        assert_eq!(idents(&copy.secondary), idents(&build.secondary));
        for (old, new) in [
            (&build.primary, &copy.primary),
            (&build.secondary, &copy.secondary),
        ] {
            for pick in &old.powers {
                assert_eq!(
                    slotted(new, &pick.internal_name),
                    pick.slots.len(),
                    "{}",
                    pick.internal_name
                );
            }
        }
        assert!(conversion.empty.is_empty());
        assert!(
            conversion
                .dropped
                .iter()
                .all(|drop| drop.set == "Weapon Mastery"),
            "only the Scrapper's epic pool stays behind: {:?}",
            conversion.dropped
        );
    }

    #[test]
    fn tanker_to_brute_swaps_the_sets() {
        let database = database();
        let build = fixture(&database, "charnel");
        let conversion = convert(&database, &build, "brute");
        let copy = &conversion.build;
        assert_eq!(copy.primary.id.as_deref(), Some("brute/dark-melee"));
        assert_eq!(copy.secondary.id.as_deref(), Some("brute/dark-armor"));
        assert_eq!(idents(&copy.primary), idents(&build.secondary));
        assert_eq!(idents(&copy.secondary), idents(&build.primary));
        assert_eq!(
            conversion
                .sets
                .iter()
                .map(|set| (set.from, set.to))
                .collect::<Vec<_>>(),
            [
                (Role::Primary, Some(Role::Secondary)),
                (Role::Secondary, Some(Role::Primary))
            ]
        );
        assert!(conversion.dropped.is_empty(), "{:?}", conversion.dropped);
    }

    #[test]
    fn blaster_to_defender_keeps_the_blast_set_as_the_secondary() {
        let database = database();
        let build = fixture(&database, "sisterachlys");
        let conversion = convert(&database, &build, "defender");
        let copy = &conversion.build;
        assert_eq!(copy.primary, PowersetSelection::default());
        assert_eq!(conversion.empty, [Role::Primary]);
        assert_eq!(copy.secondary.id.as_deref(), Some("defender/storm-blast"));
        assert_eq!(idents(&copy.secondary), idents(&build.primary));
        let ninja: Vec<&Dropped> = conversion
            .dropped
            .iter()
            .filter(|drop| drop.set == "Ninja Training")
            .collect();
        assert_eq!(ninja.len(), build.secondary.powers.len());
        assert!(ninja.iter().all(|drop| drop.why == DropWhy::SetNotShared));
    }

    /// A power the new archetype's copy of a set lacks is reported, and nothing stands in for it.
    #[test]
    fn scrapper_to_stalker_reports_powers_with_no_match() {
        let database = database();
        let build = fixture(&database, "maidenfury");
        let conversion = convert(&database, &build, "stalker");
        let unmatched: Vec<&str> = conversion
            .dropped
            .iter()
            .filter(|drop| drop.why == DropWhy::NoMatch)
            .map(|drop| drop.power.as_str())
            .collect();
        assert_eq!(unmatched, ["Dragon's Tail", "Phalanx Fighting"]);
        let copy = &conversion.build;
        for pick in copy.primary.powers.iter().chain(&copy.secondary.powers) {
            assert!(
                database
                    .find_power(&pick.powerset, &pick.internal_name)
                    .is_some(),
                "{} is a power of {}",
                pick.internal_name,
                pick.powerset
            );
            assert!(
                build
                    .all_selected()
                    .any(|old| old.internal_name == pick.internal_name),
                "{} was in the original",
                pick.internal_name
            );
        }
    }

    /// Every pick sits at or above the level its new position unlocks it, no level holds more picks
    /// than it grants, and every pick that changed level or went missing is in the report.
    #[test]
    fn every_carried_pick_lands_at_a_level_the_new_archetype_allows() {
        let database = database();
        let schedule = database.leveling_schedule.as_ref().expect("schedule");
        for (name, to) in [
            ("sisterachlys", "defender"),
            ("charnel", "brute"),
            ("maidenfury", "stalker"),
        ] {
            let build = fixture(&database, name);
            let conversion = convert(&database, &build, to);
            let copy = &conversion.build;
            let mut used: Vec<u8> = Vec::new();
            for (bucket, index) in buckets(copy) {
                let pick = &bucket_picks(copy, bucket)[index];
                if pick.level == 0 {
                    continue;
                }
                assert!(
                    pick.level >= floor(&database, bucket, pick),
                    "{name}: {} at {}",
                    pick.internal_name,
                    pick.level
                );
                used.push(pick.level);
                let old = build
                    .all_selected()
                    .find(|old| old.internal_name == pick.internal_name)
                    .expect("carried from the original");
                if old.level != pick.level {
                    assert!(
                        conversion
                            .moved
                            .iter()
                            .any(|moved| moved.from == old.level && moved.to == pick.level),
                        "{name}: {} moved without a report line",
                        pick.internal_name
                    );
                }
            }
            for level in used.iter().copied() {
                let granted = schedule
                    .pick_levels()
                    .iter()
                    .filter(|&&l| l == level)
                    .count();
                assert!(
                    used.iter().filter(|&&l| l == level).count() <= granted,
                    "{name}: level {level}"
                );
            }
            let carried = buckets(copy).len();
            let original = buckets(&build).len();
            assert_eq!(
                carried + conversion.dropped.len(),
                original,
                "{name}: every pick accounted for"
            );
        }

        // The nuke: 26 on a Blaster's primary, later on a Defender's secondary.
        let build = fixture(&database, "sisterachlys");
        let conversion = convert(&database, &build, "defender");
        let nuke = conversion
            .moved
            .iter()
            .find(|moved| moved.power == "Category Five")
            .expect("the nuke moved");
        assert_eq!(nuke.from, 26);
        let unlock = database
            .find_power("defender/storm-blast", "Category_Five")
            .expect("power")
            .unlock_level();
        assert!(
            nuke.to >= unlock && unlock > 26,
            "{nuke:?}, unlocks at {unlock}"
        );
    }

    /// A pick moves only when it must: its old level is below its new unlock level, or a pick that
    /// had to move took its place.
    #[test]
    fn a_conversion_moves_only_what_it_must() {
        let database = database();
        let build = fixture(&database, "maidenfury");
        assert!(convert(&database, &build, "brute").moved.is_empty());

        let build = fixture(&database, "charnel");
        let conversion = convert(&database, &build, "brute");
        let copy = &conversion.build;
        let had_to: Vec<&Moved> = conversion
            .moved
            .iter()
            .filter(|moved| {
                buckets(copy).into_iter().any(|(bucket, index)| {
                    let pick = &bucket_picks(copy, bucket)[index];
                    display_name(&database, pick) == moved.power
                        && floor(&database, bucket, pick) > moved.from
                })
            })
            .collect();
        assert!(!had_to.is_empty());
        for moved in &conversion.moved {
            let forced = had_to.contains(&moved);
            let made_room = had_to.iter().any(|other| other.to == moved.from);
            assert!(
                forced || made_room,
                "{moved:?} moved for nothing: {:?}",
                conversion.moved
            );
        }
    }

    /// A move never puts a power before the one it requires. Weave (after two of Boxing, Kick and
    /// Tough) and Tough sit at levels that grant no pick, so both must move; the nearest free picks
    /// are 16 for Weave and 22 for Tough, which would put Weave ahead of Tough.
    #[test]
    fn a_move_keeps_prerequisites_first() {
        let database = database();
        let mut build = fixture(&database, "charnel");
        let level = |build: &CharacterState, ident: &str| {
            build
                .all_selected()
                .find(|pick| pick.internal_name == ident)
                .map(|pick| pick.level)
        };
        assert_eq!(
            (level(&build, "Tough"), level(&build, "Weave")),
            (Some(16), Some(22))
        );
        for pick in build
            .pools
            .iter_mut()
            .flat_map(|pool| pool.powers.iter_mut())
        {
            match pick.internal_name.as_str() {
                "Tough" => pick.level = 23,
                "Weave" => pick.level = 17,
                _ => {}
            }
        }
        let conversion = convert(&database, &build, "brute");
        let copy = &conversion.build;
        assert!(
            level(copy, "Weave") > level(copy, "Tough"),
            "{:?}",
            conversion.moved
        );
        for pick in copy.all_selected() {
            if !conversion
                .moved
                .iter()
                .any(|moved| moved.power == display_name(&database, pick))
            {
                continue;
            }
            let power = database
                .resolve_power(&pick.powerset, &pick.internal_name)
                .expect("power");
            let gate = coh_data::granted_powers::requires(power).unwrap_or_default();
            assert_eq!(
                coh_data::requires_met_in_order(
                    &gate,
                    copy,
                    pick.level,
                    Some("brute"),
                    &database.set_paths
                ),
                Ok(true),
                "{} at {}: {:?}",
                pick.internal_name,
                pick.level,
                conversion.moved
            );
        }
    }

    /// A Blaster's own set pieces, its epic pool and Defiance stay behind and are named; the power
    /// pools and the Incarnate powers come across and are not.
    #[test]
    fn blaster_to_defender_lists_what_could_not_come() {
        let database = database();
        let mut build = fixture(&database, "sisterachlys");
        // The fixture slots Blaster's Wrath in a Ninja Training power, which goes with its set;
        // hand the pieces to the nuke so the piece check has them to refuse.
        let wrath = build
            .secondary
            .powers
            .iter()
            .find(|pick| pick.internal_name == "The_Lotus_Drops")
            .expect("The Lotus Drops")
            .slots
            .clone();
        build
            .primary
            .powers
            .iter_mut()
            .find(|pick| pick.internal_name == "Category_Five")
            .expect("Category Five")
            .slots = wrath;
        let conversion = convert(&database, &build, "defender");

        let wrath: Vec<&DroppedPiece> = conversion
            .pieces
            .iter()
            .filter(|piece| piece.set.as_deref() == Some("Superior Blaster's Wrath"))
            .collect();
        assert_eq!(wrath.len(), 6, "{:?}", conversion.pieces);
        assert!(wrath.iter().all(|piece| piece.power == "Category Five"));
        let nuke = conversion
            .build
            .secondary
            .powers
            .iter()
            .find(|pick| pick.internal_name == "Category_Five")
            .expect("carried");
        assert_eq!(nuke.slots.len(), 6, "the slots stay, empty");
        assert!(nuke.slots.iter().all(Option::is_none));

        let epic: Vec<&str> = conversion
            .dropped
            .iter()
            .filter(|drop| drop.why == DropWhy::Refused)
            .map(|drop| drop.set.as_str())
            .collect();
        assert!(
            !epic.is_empty() && epic.iter().all(|set| *set == "Ice Mastery"),
            "{epic:?}"
        );
        assert_eq!(conversion.build.epic_pool, None);
        assert_eq!(conversion.inherent.as_deref(), Some("Defiance"));

        assert!(
            conversion
                .dropped
                .iter()
                .all(|drop| !build.pools.iter().any(|pool| pool
                    .powers
                    .iter()
                    .any(|pick| display_name(&database, pick) == drop.power))),
            "no power pool pick is listed: {:?}",
            conversion.dropped
        );
        assert_eq!(conversion.build.pools.len(), build.pools.len());
        assert_eq!(conversion.build.incarnates, build.incarnates);
    }
}
