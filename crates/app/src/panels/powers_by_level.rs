//! The Powers panel's by-level layout — the build's picks laid out over the levels the
//! game grants them at, instead of grouped by the powerset they came from (the beta's
//! `ChronologicalPowerView`, its "By Level" mode). Same cards, same data, different
//! arrangement: [`crate::panels::powers::PickedPowerCard`] renders every occupied cell,
//! so a power looks and behaves identically in both layouts.
//!
//! Every level in the grid comes from the dataset's own
//! [`LevelingSchedule::pick_levels`](coh_data::LevelingSchedule::pick_levels) — one cell
//! per pick the character earns, in ascending order — never a hand-written list of pick
//! levels (the beta hardcodes three arrays of eight).
//!
//! A pick moves by dragging its level badge onto another cell, and WHICH BAND of that cell it is
//! dropped on decides what happens ([`DropMode`]):
//!
//! - the **middle third** of a power's card **swaps** the two levels, leaving every pick between
//!   them where it is — the beta's drag-and-swap, for when only the two are wrong;
//! - the **top or bottom third** of the card **inserts before or after** that power: every pick
//!   between the two slides one cell toward the cell it left, so dropping a level-47 power above
//!   level 18 pushes level 18 and the powers under it one pick later. That is how a build gets
//!   re-planned without un-picking and re-slotting everything below the change.
//!
//! The two bands exist because the gap between cards is 3px (`gap` on `.power-column`), too
//! small to aim at with a mouse and unusable on touch: the card's edge stands in for "between
//! this power and the next". Both operations share the same targets, so neither needs a
//! separate affordance.
//!
//! Only the stored `level` changes, so slots, enhancements and toggles travel with the
//! power, and the slot-level solver re-houses any slot the new level can no longer serve.

use crate::build_session::BuildSession;
use crate::panels::power_group::PowerGroup;
use crate::panels::powers::{set_unlock_floor, PickedPowerCard, SetOrigin};
use crate::shell::Db;
use crate::view::power_view::resolve_power_def;
use coh_data::{CharacterState, SelectedPower};
use dioxus::prelude::*;

/// Columns the picks are dealt into, left to right. Three is the beta's shape and what
/// the ~24 picks of every shipped dataset divide into evenly; the column *contents* are
/// derived, so a dataset granting a different number of picks still fills them.
const COLUMNS: usize = 3;

/// Below this much travel a press on the badge is not a drag.
const PICK_DRAG_THRESHOLD_PX: f64 = 4.0;

/// One cell of the grid: a pick the character earns at `level`, and the power occupying
/// it (`None` = the pick is unspent).
#[derive(Clone, PartialEq)]
struct PickCell {
    level: u8,
    power: Option<SelectedPower>,
}

/// Deal the build's picked powers onto the schedule's picks.
///
/// Powers are placed lowest-level first (ties in `picked_powers` traversal order:
/// primary → secondary → pools → epic), each into the first free cell whose level is its
/// own, then into the first free cell at a higher level. A power that fits nowhere is
/// returned separately rather than being forced into a cell the game wouldn't grant it —
/// the caller shows it as a visible fault (Rule 1), which is how a build carrying levels
/// this schedule never grants announces itself.
fn deal_picks(
    pick_levels: &[u8],
    mut powers: Vec<SelectedPower>,
) -> (Vec<PickCell>, Vec<SelectedPower>) {
    let mut cells: Vec<PickCell> = pick_levels
        .iter()
        .map(|&level| PickCell { level, power: None })
        .collect();
    let mut unplaced = Vec::new();

    powers.sort_by_key(|power| power.level);
    for power in powers {
        let free_at = |cells: &[PickCell], level: u8| {
            cells
                .iter()
                .position(|cell| cell.power.is_none() && cell.level == level)
        };
        let free_above = |cells: &[PickCell], level: u8| {
            cells
                .iter()
                .position(|cell| cell.power.is_none() && cell.level > level)
        };
        match free_at(&cells, power.level).or_else(|| free_above(&cells, power.level)) {
            Some(index) => cells[index].power = Some(power),
            None => unplaced.push(power),
        }
    }

    (cells, unplaced)
}

/// A column's heading — the band of levels its cells cover. Derived from the cells
/// themselves so it can never drift from what the column shows.
fn column_title(cells: &[PickCell]) -> String {
    match (cells.first(), cells.last()) {
        (Some(first), Some(last)) if first.level != last.level => {
            format!("Levels {}–{}", first.level, last.level)
        }
        (Some(only), _) => format!("Level {}", only.level),
        _ => String::new(),
    }
}

/// One pick, addressed by the set it came from and its internal name. Both halves, because
/// `internalName` repeats across sets (Build_Up ×64), and the set on the pick itself rather
/// than the bucket's id, because a VEAT branch pick sits in a role list no bucket id names.
#[derive(Clone, PartialEq, Debug)]
struct PickKey {
    powerset: String,
    internal_name: String,
}

impl PickKey {
    fn of(power: &SelectedPower) -> Self {
        PickKey {
            powerset: power.powerset.clone(),
            internal_name: power.internal_name.clone(),
        }
    }
}

/// A pick about to be moved: who it is, the level it holds now, and the lowest level the game
/// would let it be taken at.
#[derive(Clone, PartialEq, Debug)]
struct MovingPick {
    key: PickKey,
    /// The power's display name, for the refusal text.
    name: String,
    level: u8,
    /// `None` when the power does not resolve against this dataset, so no shift may carry it.
    floor: Option<u8>,
}

/// The pick-spending power `key` names, wherever the build keeps it.
fn pick_mut<'a>(state: &'a mut CharacterState, key: &PickKey) -> Option<&'a mut SelectedPower> {
    state
        .primary
        .powers
        .iter_mut()
        .chain(state.secondary.powers.iter_mut())
        .chain(
            state
                .pools
                .iter_mut()
                .flat_map(|pool| pool.powers.iter_mut()),
        )
        .chain(
            state
                .epic_pool
                .iter_mut()
                .flat_map(|epic| epic.powers.iter_mut()),
        )
        .filter(|power| !power.is_locked)
        .find(|power| power.powerset == key.powerset && power.internal_name == key.internal_name)
}

/// The lowest level `power` can be taken at: its own unlock level, raised to its set's floor
/// (pools and epics open as a whole before any power in them does). The same two terms the
/// picker's rows floor on, so a drag can never place a power where a click couldn't have.
/// `None` when the def can't be resolved — a move the planner can't check is refused.
fn pick_floor(database: &Db, state: &CharacterState, power: &SelectedPower) -> Option<u8> {
    let def = resolve_power_def(database, &power.powerset, &power.internal_name)?;
    let origin = if state.pools.iter().any(|pool| pool.id == power.powerset) {
        SetOrigin::Pool
    } else if state
        .epic_pool
        .as_ref()
        .is_some_and(|epic| epic.id == power.powerset)
    {
        SetOrigin::Epic
    } else {
        SetOrigin::Powerset
    };
    Some(
        def.unlock_level()
            .max(set_unlock_floor(database, &power.powerset, origin)),
    )
}

/// The name a player knows `power` by, falling back to its internal name when the def is gone.
fn display_name(database: &Db, power: &SelectedPower) -> String {
    resolve_power_def(database, &power.powerset, &power.internal_name)
        .map_or_else(|| power.internal_name.clone(), |def| def.name.clone())
}

/// What release on a cell would do to the build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DropMode {
    /// Trade levels with the target cell (or take its level when the cell is unspent).
    Swap,
    /// Remove from `from` and insert at `target`, sliding the run between them one pick along.
    Insert,
}

/// Which third of a card the pointer is in.
///
/// Two operations share the same card targets: the middle third swaps with that card, and the
/// top and bottom thirds insert before or after it. The bands are the card's thirds rather
/// than only the 3px gap between cards (`gap: 3px` on `.power-column`), because a 3px sliver
/// cannot be aimed at reliably and is unusable on touch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CellBand {
    Before,
    Middle,
    After,
}

impl CellBand {
    /// Which band a pointer at `y` inside `top..bottom` falls in.
    ///
    /// Boundaries belong to the outer bands so aiming at an edge is forgiving; a zero-height
    /// box before layout lands falls back to `Middle`, which touches at most two picks.
    fn at(y: f64, top: f64, bottom: f64) -> CellBand {
        let height = bottom - top;
        if height <= 0.0 {
            return CellBand::Middle;
        }
        let third = height / 3.0;
        if y <= top + third {
            CellBand::Before
        } else if y >= bottom - third {
            CellBand::After
        } else {
            CellBand::Middle
        }
    }

    /// The destination cell and [`DropMode`] this band on `cell` means when dragging `from`.
    ///
    /// `After` cell `k` and `Before` cell `k + 1` both name seam `k + 1`, so crossing the
    /// boundary between two adjacent powers never flips where the dragged power lands.
    fn resolve(self, from: usize, cell: usize) -> (usize, DropMode) {
        match self {
            CellBand::Middle => (cell, DropMode::Swap),
            CellBand::Before => (insert_target(from, cell), DropMode::Insert),
            CellBand::After => (insert_target(from, cell + 1), DropMode::Insert),
        }
    }
}

/// Destination cell index when removing cell `from` and inserting at seam `slot` (`0..=len`,
/// where `slot` is the seam before cell `slot` and `len` is after the final cell).
fn insert_target(from: usize, slot: usize) -> usize {
    if slot <= from {
        slot
    } else {
        slot - 1
    }
}

/// Where each cell's pick comes from for an [`DropMode::Insert`] drop of cell `from` on cell
/// `to`: destination cell → source cell. A remove-then-insert over the cells, so the whole run
/// between the two slides one cell toward the cell the pick left.
///
/// Empty cells travel with their position and carry nothing, which is what makes a drop that
/// crosses no power behave as the plain "move to that level" it looks like, while a drop that
/// crosses powers brings them along.
fn shift_sources(len: usize, from: usize, to: usize) -> Vec<usize> {
    let mut sources: Vec<usize> = (0..len).collect();
    if from == to || from >= len || to >= len {
        return sources;
    }
    let moved = sources.remove(from);
    sources.insert(to, moved);
    sources
}

/// The cells a drop on `to` pushes to another level: everything the shift rearranges, other
/// than the cell the pick left (drawn as the source) and the cell it lands in (the target).
fn carried_cells(len: usize, from: usize, to: usize) -> Vec<usize> {
    shift_sources(len, from, to)
        .iter()
        .enumerate()
        .filter(|(destination, source)| {
            **source != *destination && *destination != to && *destination != from
        })
        .map(|(destination, _)| destination)
        .collect()
}

/// Give every pick the level of the cell it arrives at, for an [`DropMode::Insert`] drop.
/// Nothing else on any pick changes, so slots, enhancements and toggles travel with the power.
fn apply_shift(state: &mut CharacterState, cells: &[PickCell], from: usize, to: usize) {
    for (destination, &source) in shift_sources(cells.len(), from, to).iter().enumerate() {
        let Some(power) = cells.get(source).and_then(|cell| cell.power.as_ref()) else {
            continue;
        };
        let level = cells[destination].level;
        if power.level == level {
            continue;
        }
        if let Some(power) = pick_mut(state, &PickKey::of(power)) {
            power.level = level;
        }
    }
}

/// Apply a [`DropMode::Swap`] drop from `from` onto `to`: trade levels with the occupant when
/// `to` holds a power, or move `from` to `to`'s level when `to` is unspent. Every other pick
/// keeps its level, so nothing between the two moves.
fn apply_swap(state: &mut CharacterState, cells: &[PickCell], from: usize, to: usize) {
    let Some(dragged) = cells.get(from).and_then(|cell| cell.power.as_ref()) else {
        return;
    };
    let Some(to_cell) = cells.get(to) else {
        return;
    };
    let from_level = dragged.level;
    let to_level = to_cell.level;
    if let Some(occupant) = to_cell.power.as_ref() {
        if occupant.level != from_level {
            if let Some(power) = pick_mut(state, &PickKey::of(occupant)) {
                power.level = from_level;
            }
        }
    }
    if dragged.level != to_level {
        if let Some(power) = pick_mut(state, &PickKey::of(dragged)) {
            power.level = to_level;
        }
    }
}

/// Why dropping the pick in cell `from` on cell `to` is refused, or `None` when the shift is
/// legal.
///
/// A shift moves every pick between the two cells, so the rules are applied to all of them and
/// not only to the dragged one. They are the picker's own:
/// - every arriving pick lands at or above its own floor ([`pick_floor`]) — the term that
///   refuses a drop reaching back for a late power, by naming the power it would pull below
///   its unlock level;
/// - level 1 holds one primary pick and one secondary pick, never two of either
///   (`bucket_owns_level_one` in the picker);
/// - in level-up mode, nothing lands above the character's current level.
fn drop_refusal(
    state: &CharacterState,
    cells: &[PickCell],
    picks: &[Option<MovingPick>],
    from: usize,
    to: usize,
    mode: DropMode,
    level_cap: Option<u8>,
) -> Option<String> {
    if from == to {
        return Some("Already at this level".to_string());
    }
    // A middle-band drop moves at most the two ends (just the dragged pick when `to` is
    // unspent), so only those endpoints are checked against their floors.
    if mode == DropMode::Swap {
        let to_level = cells[to].level;
        let from_level = cells[from].level;
        if to_level == from_level {
            return Some("Already at this level".to_string());
        }
        let Some(dragged) = picks.get(from).and_then(Option::as_ref) else {
            return Some(
                "This power isn't in this dataset, so its unlock level is unknown".to_string(),
            );
        };
        let check_end = |pick: &MovingPick, level: u8| -> Option<String> {
            let Some(floor) = pick.floor else {
                return Some(format!(
                    "{} isn't in this dataset, so its unlock level is unknown",
                    pick.name
                ));
            };
            if level < floor {
                return Some(format!(
                    "{} unlocks at level {}, so it can't be taken at {level}",
                    pick.name, floor
                ));
            }
            if let Some(cap) = level_cap.filter(|&cap| level > cap) {
                return Some(format!(
                    "Level Up mode is on — the character is level {cap}"
                ));
            }
            None
        };
        if let Some(reason) = check_end(dragged, to_level) {
            return Some(reason);
        }
        if let Some(occupant) = picks.get(to).and_then(Option::as_ref) {
            if let Some(reason) = check_end(occupant, from_level) {
                return Some(reason);
            }
        }
        let mut trial = state.clone();
        apply_swap(&mut trial, cells, from, to);
        return crowded_level_one(&trial)
            .then(|| "Level 1 holds one primary pick and one secondary pick".to_string());
    }
    for (destination, &source) in shift_sources(cells.len(), from, to).iter().enumerate() {
        if source == destination {
            continue;
        }
        let Some(pick) = picks.get(source).and_then(Option::as_ref) else {
            continue;
        };
        let level = cells[destination].level;
        let Some(floor) = pick.floor else {
            return Some(format!(
                "{} isn't in this dataset, so its unlock level is unknown",
                pick.name
            ));
        };
        if level < floor {
            return Some(format!(
                "{} unlocks at level {}, so it can't be taken at {level}",
                pick.name, floor
            ));
        }
        if let Some(cap) = level_cap.filter(|&cap| level > cap) {
            return Some(format!(
                "Level Up mode is on — the character is level {cap}"
            ));
        }
    }
    // Trial the shift and count, rather than reasoning about which bucket each pick is in: a
    // VEAT branch pick sits in a role list under its own set id, and the count over the role
    // list is the one `bucket_owns_level_one` takes.
    let mut trial = state.clone();
    apply_shift(&mut trial, cells, from, to);
    crowded_level_one(&trial)
        .then(|| "Level 1 holds one primary pick and one secondary pick".to_string())
}

/// Does either bucket hold two picks at level 1 — the count `bucket_owns_level_one` in the
/// picker keeps at one each?
fn crowded_level_one(state: &CharacterState) -> bool {
    let crowded = |powers: &[SelectedPower]| {
        powers
            .iter()
            .filter(|power| !power.is_locked && power.level == 1)
            .count()
            > 1
    };
    crowded(&state.primary.powers) || crowded(&state.secondary.powers)
}

/// A drag in flight: which cell it started from, where the pointer is, and what dropping on
/// each cell would do — decided once at the press, because nothing in the build changes until
/// release.
#[derive(Clone, PartialEq)]
struct PickDrag {
    from_cell: usize,
    origin: (f64, f64),
    moved: bool,
    /// Per cell, `None` when a drop there is legal, else why not — one answer per operation,
    /// because which one a release means depends on where in the cell the pointer is.
    refusals: Vec<(Option<String>, Option<String>)>,
    /// The cells' on-screen boxes `(index, left, top, right, bottom)`, measured once the press's
    /// DOM round trip lands. Empty until then, so a drop that beats the measurement does nothing.
    rects: Vec<(usize, f64, f64, f64, f64)>,
    /// The cell under the pointer, and which band of it the pointer is in.
    hover: Option<(usize, CellBand)>,
}

impl PickDrag {
    /// The cell under the pointer and the band its position names.
    ///
    /// Also claims the 3px vertical gap between two stacked cards in the same column, snapping
    /// it to the lower card's `Before` band (the same seam as the upper card's `After` band),
    /// so moving across the seam between two powers never drops `hover` to `None`.
    fn band_at(&self, x: f64, y: f64) -> Option<(usize, CellBand)> {
        if let Some(&(index, _, top, _, bottom)) =
            self.rects.iter().find(|(_, left, top, right, bottom)| {
                x >= *left && x <= *right && y >= *top && y <= *bottom
            })
        {
            return Some((index, CellBand::at(y, top, bottom)));
        }
        self.rects.windows(2).find_map(|pair| {
            let &(_, left, _, right, upper_bottom) = &pair[0];
            let &(lower_index, _, lower_top, _, _) = &pair[1];
            let gap = lower_top - upper_bottom;
            (x >= left
                && x <= right
                && (0.0..=12.0).contains(&gap)
                && y >= upper_bottom
                && y <= lower_top)
                .then_some((lower_index, CellBand::Before))
        })
    }

    /// The reason a drop on `cell` in `mode` is refused, or `None` when it is legal. The press
    /// decided both operations' verdicts, so a release only has to look one up.
    fn refusal(&self, cell: usize, mode: DropMode) -> Option<String> {
        self.refusals
            .get(cell)
            .and_then(|(swap, insert)| match mode {
                DropMode::Swap => swap.clone(),
                DropMode::Insert => insert.clone(),
            })
    }
}

/// Capture the pointer on the pressed badge and measure every cell of the visible grid.
///
/// Through `eval`, as `arm_slot_drag` does it, so the desktop webview gets the same gesture as
/// the browser. Capture is what keeps `pointermove`/`pointerup` arriving on the badge once the
/// pointer leaves it, and on touch it is what stops the finger's drag scrolling the panel. With
/// the pointer captured no cell sees it pass, so the drop target is found by hit-testing these
/// boxes. Both layout roots are in the document at once, so only cells that render are kept.
async fn arm_pick_drag(cell: usize, pointer_id: i32) -> Vec<(usize, f64, f64, f64, f64)> {
    let js = format!(
        r#"
        const grips = document.querySelectorAll('[data-pick-grip="{cell}"]');
        const grip = Array.from(grips).find(g => g.getBoundingClientRect().width > 0);
        if (grip) {{ try {{ grip.setPointerCapture({pointer_id}); }} catch (_) {{}} }}
        return Array.from(document.querySelectorAll('[data-pick-cell]'))
            .map(c => [Number(c.dataset.pickCell), c.getBoundingClientRect()])
            .filter(([_, r]) => r.width > 0)
            .map(([i, r]) => [i, r.left, r.top, r.right, r.bottom]);
        "#
    );
    let Ok(value) = document::eval(&js).await else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let n = |i: usize| row.get(i).and_then(|v| v.as_f64());
                    Some((n(0)? as usize, n(1)?, n(2)?, n(3)?, n(4)?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The by-level grid: one cell per power pick, dealt into columns.
#[component]
pub fn PowersByLevel(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let level_up_mode = use_context::<crate::level_control::LevelUpMode>().0;
    let drag = use_signal(|| Option::<PickDrag>::None);

    // No schedule ⇒ no picks to lay out, and inventing a level ladder here is exactly the
    // hardcode Rule 0 forbids. Say so instead (Rule 1).
    let Some(schedule) = database.leveling_schedule.as_ref() else {
        return rsx! {
            div { class: "load-state error",
                "This dataset carries no leveling schedule, so the levels powers are picked at are unknown."
            }
        };
    };

    let picked: Vec<SelectedPower> = session.build.read().picked_powers().cloned().collect();
    // The slot the next pick goes to (the clicked one, or the earliest empty), drawn as the
    // current cell — free-form only, where empty cells are clickable to move it.
    let mut target_slot = use_context::<crate::level_control::TargetSlot>().0;
    let taken_levels: Vec<u8> = picked.iter().map(|power| power.level).collect();
    let working_level = (!level_up_mode())
        .then(|| crate::level_control::working_level_of(schedule, &taken_levels, target_slot()))
        .flatten();
    let (cells, unplaced) = deal_picks(&schedule.pick_levels(), picked);
    // Level 1 holds two picks; only the first empty one is the current cell.
    let working_cell = working_level.and_then(|level| {
        cells
            .iter()
            .position(|cell| cell.power.is_none() && cell.level == level)
    });
    // Ceil-divide so the last column is the short one, never an extra column of one.
    let per_column = cells.len().div_ceil(COLUMNS).max(1);

    let active = drag().filter(|drag| drag.moved);
    // What release would do, said under the grid while a drag is live — the one place a refusal
    // reason can be read mid-gesture.
    let drag_status = active.as_ref().map(|drag| match drag.hover {
        None => "Drop on a power's middle to swap levels, or on its top or bottom edge to insert"
            .to_string(),
        Some((cell, band)) => {
            let (target, mode) = band.resolve(drag.from_cell, cell);
            match drag.refusal(target, mode) {
                Some(reason) => reason,
                None => {
                    let to_level = cells[target].level;
                    match mode {
                        DropMode::Swap => match &cells[target].power {
                            Some(occupant) => {
                                format!("Swap levels with {}", display_name(&database, occupant))
                            }
                            None => format!("Move to level {to_level}"),
                        },
                        DropMode::Insert => {
                            let carried = carried_cells(cells.len(), drag.from_cell, target)
                                .into_iter()
                                .filter(|&index| cells[index].power.is_some())
                                .count();
                            match carried {
                                0 => format!("Insert at level {to_level}"),
                                1 => format!(
                                    "Insert at level {to_level}, and 1 power to its next pick"
                                ),
                                n => format!(
                                    "Insert at level {to_level}, and {n} powers to their next pick"
                                ),
                            }
                        }
                    }
                }
            }
        }
    });

    rsx! {
        div { class: "power-columns",
            for (column_index, column) in cells.chunks(per_column).enumerate() {
                PowerGroup {
                    key: "col-{column_index}",
                    title: column_title(column),
                    // The picks in the band, spent or not — the count a title like
                    // "Levels 1–4" names, and the number of cells folding away.
                    count: column.len(),
                    for (cell_offset, cell) in column.iter().enumerate() {
                        {
                            let index = column_index * per_column + cell_offset;
                            // A cell shows what the pointer's band on it will actually commit:
                            // middle outlines and fills the card for a swap, top/bottom lights
                            // the seam before or after it for an insert, and a refused band
                            // dims the cell rather than promising a move release will reject.
                            let band_legal = |drag: &PickDrag, band: CellBand| {
                                let (target, mode) = band.resolve(drag.from_cell, index);
                                drag.refusal(target, mode).is_none()
                            };
                            let cell_class = match &active {
                                Some(drag) if drag.from_cell == index => {
                                    "pick-cell pick-cell--source"
                                }
                                Some(drag) => {
                                    let hovered_band = drag
                                        .hover
                                        .filter(|(cell, _)| *cell == index)
                                        .map(|(_, band)| band);
                                    match hovered_band {
                                        Some(band) if band_legal(drag, band) => match band {
                                            CellBand::Middle => {
                                                "pick-cell pick-cell--target pick-cell--hover pick-cell--swap"
                                            }
                                            CellBand::Before => {
                                                "pick-cell pick-cell--target pick-cell--insert pick-cell--insert-before"
                                            }
                                            CellBand::After => {
                                                "pick-cell pick-cell--target pick-cell--insert pick-cell--insert-after"
                                            }
                                        },
                                        Some(_) => "pick-cell pick-cell--refused",
                                        None if [
                                            CellBand::Middle,
                                            CellBand::Before,
                                            CellBand::After,
                                        ]
                                        .into_iter()
                                        .any(|band| band_legal(drag, band)) =>
                                        {
                                            "pick-cell pick-cell--target"
                                        }
                                        None => "pick-cell pick-cell--refused",
                                    }
                                }
                                None => "pick-cell",
                            };
                            rsx! {
                                div {
                                    key: "pick-{index}",
                                    class: cell_class,
                                    "data-pick-cell": "{index}",
                                    if let Some(power) = &cell.power {
                                        PickedPowerCard {
                                            database: database.clone(),
                                            power: power.clone(),
                                            powerset_id: power.powerset.clone(),
                                            level_badge: rsx! {
                                                PickGrip {
                                                    database: database.clone(),
                                                    cells: cells.clone(),
                                                    index,
                                                    drag,
                                                    level_cap: level_up_mode().then(|| session.build.read().level),
                                                }
                                            },
                                        }
                                    } else if level_up_mode() {
                                        div { class: "power-pick--empty",
                                            span { class: "power-level", "L{cell.level}" }
                                            span { class: "power-pick-hint", "Unspent pick" }
                                        }
                                    } else {
                                        // Free-form: a click aims the next pick at this slot
                                        // (Mids' behaviour). Clicking the aimed slot again lets
                                        // go, back to the earliest empty one.
                                        button {
                                            class: if working_cell == Some(index) {
                                                "power-pick--empty is-working"
                                            } else {
                                                "power-pick--empty"
                                            },
                                            title: if working_cell == Some(index) {
                                                "The next power you take goes here, if it can be taken at level {cell.level}"
                                            } else {
                                                "Click to put the next power you take at level {cell.level}"
                                            },
                                            onclick: {
                                                let level = cell.level;
                                                move |_| {
                                                    let aimed = target_slot() == Some(level);
                                                    target_slot.set((!aimed).then_some(level));
                                                }
                                            },
                                            span { class: "power-level", "L{cell.level}" }
                                            span { class: "power-pick-hint",
                                                if working_cell == Some(index) { "Next pick" } else { "Unspent pick" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(status) = drag_status {
            div { class: "pick-drag-status", "{status}" }
        }
        if !unplaced.is_empty() {
            div { class: "load-state error",
                "{unplaced.len()} power(s) sit at a level this dataset grants no pick at: "
                {unplaced.iter().map(|p| p.internal_name.as_str()).collect::<Vec<_>>().join(", ")}
            }
        }
    }
}

/// A picked card's level badge in the by-level grid, doubling as the handle that moves the
/// pick. Press and drag it onto another cell; release commits the move as one undo step.
#[component]
fn PickGrip(
    database: Db,
    cells: Vec<PickCell>,
    index: usize,
    drag: Signal<Option<PickDrag>>,
    level_cap: Option<u8>,
) -> Element {
    let session = use_context::<BuildSession>();
    let Some(power) = cells.get(index).and_then(|cell| cell.power.clone()) else {
        return rsx! {};
    };
    let mut drag = drag;

    rsx! {
        span {
            class: "power-level pick-grip",
            title: "Drag onto another power's middle to swap levels, or onto its top or bottom edge to insert before or after it — everything in between slides one pick along.",
            "data-pick-grip": "{index}",
            onpointerdown: {
                let database = database.clone();
                let cells = cells.clone();
                move |evt: Event<PointerData>| {
                    evt.prevent_default();
                    let state = session.build.read();
                    // Every cell's verdict is fixed for the whole gesture, so it is decided here
                    // once rather than on every pointer move. A shift moves the whole run
                    // between the two cells, so each pick's floor is resolved up front and the
                    // verdict for a target is decided over all of them.
                    let picks: Vec<Option<MovingPick>> = cells
                        .iter()
                        .map(|cell| {
                            cell.power.as_ref().map(|other| MovingPick {
                                key: PickKey::of(other),
                                name: display_name(&database, other),
                                level: other.level,
                                floor: pick_floor(&database, &state, other),
                            })
                        })
                        .collect();
                    // Both operations' verdicts, per cell: which one a release means is decided
                    // by where in the card the pointer is, which the press cannot know yet.
                    let refusals = (0..cells.len())
                        .map(|target| {
                            let verdict = |mode| {
                                drop_refusal(&state, &cells, &picks, index, target, mode, level_cap)
                            };
                            (verdict(DropMode::Swap), verdict(DropMode::Insert))
                        })
                        .collect();
                    drop(state);
                    let coordinates = evt.client_coordinates();
                    drag.set(Some(PickDrag {
                        from_cell: index,
                        origin: (coordinates.x, coordinates.y),
                        moved: false,
                        refusals,
                        rects: Vec::new(),
                        hover: None,
                    }));
                    // Returned as a future for Dioxus to drive, not spawned: see `arm_slot_drag`.
                    let pointer_id = evt.pointer_id();
                    async move {
                        let rects = arm_pick_drag(index, pointer_id).await;
                        drag.with_mut(|active| {
                            if let Some(active) = active.as_mut() {
                                active.rects = rects;
                            }
                        });
                    }
                }
            },
            onpointermove: move |evt: Event<PointerData>| {
                let point = evt.client_coordinates();
                drag.with_mut(|active| {
                    let Some(active) = active.as_mut() else { return };
                    let travel = (point.x - active.origin.0).hypot(point.y - active.origin.1);
                    if !active.moved && travel > PICK_DRAG_THRESHOLD_PX {
                        active.moved = true;
                    }
                    active.hover = active.band_at(point.x, point.y);
                });
            },
            onpointerup: {
                let database = database.clone();
                let cells = cells.clone();
                move |_| {
                    let Some(finished) = drag.take() else { return };
                    let Some((cell, band)) = finished.hover.filter(|_| finished.moved) else {
                        return;
                    };
                    let (target, mode) = band.resolve(finished.from_cell, cell);
                    if finished.refusal(target, mode).is_some() {
                        return;
                    }
                    let database = database.clone();
                    let cells = cells.clone();
                    session.commit(move |state| {
                        // Floors were checked at the press; the move itself needs none.
                        match mode {
                            DropMode::Swap => apply_swap(state, &cells, index, target),
                            DropMode::Insert => apply_shift(state, &cells, index, target),
                        }
                        // A grant gated on a pick's level follows the pick in the same edit.
                        crate::granted_powers::sync(state, &database);
                    });
                }
            },
            onpointercancel: move |_| drag.set(None),
            onlostpointercapture: move |_| drag.set(None),
            "L{power.level}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::DatasetId;

    fn pick(set: &str, name: &str, level: u8) -> SelectedPower {
        SelectedPower::picked(name, set, level)
    }

    fn moving(power: &SelectedPower, floor: u8) -> MovingPick {
        MovingPick {
            key: PickKey::of(power),
            name: power.internal_name.clone(),
            level: power.level,
            floor: Some(floor),
        }
    }

    fn level_of(state: &CharacterState, set: &str, name: &str) -> u8 {
        state
            .picked_powers()
            .find(|p| p.powerset == set && p.internal_name == name)
            .unwrap()
            .level
    }

    /// Homecoming's real pick ladder: two picks at level 1, then one at each level from 2 to
    /// 10. Twelve cells, so a run between any two of them has something in it.
    const LADDER: [u8; 12] = [1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

    /// The grid over [`LADDER`], with `state`'s powers dealt onto it. Each pick floors at 1
    /// unless a test raises it, so nothing refuses for a reason the test did not name.
    fn grid(state: &CharacterState) -> (Vec<PickCell>, Vec<Option<MovingPick>>) {
        let (cells, unplaced) = deal_picks(&LADDER, state.picked_powers().cloned().collect());
        assert!(unplaced.is_empty(), "every power fits the ladder");
        let picks = cells
            .iter()
            .map(|cell| cell.power.as_ref().map(|power| moving(power, 1)))
            .collect();
        (cells, picks)
    }

    /// A build with enough picks to fill a run: three primary, two secondary, two pool.
    fn populated() -> CharacterState {
        let mut state = CharacterState::empty(DatasetId::Homecoming);
        state.primary.id = Some("Prim".into());
        state.primary.powers = vec![
            pick("Prim", "P1", 1),
            pick("Prim", "P2", 2),
            pick("Prim", "P3", 6),
        ];
        state.secondary.id = Some("Sec".into());
        state.secondary.powers = vec![pick("Sec", "S1", 1), pick("Sec", "S2", 4)];
        state.pools.push(coh_data::PoolSelection {
            id: "Pool".into(),
            name: "Pool".into(),
            powers: vec![pick("Pool", "Hover", 8), pick("Pool", "Fly", 10)],
        });
        state
    }

    fn cell_of(cells: &[PickCell], set: &str, name: &str) -> usize {
        cells
            .iter()
            .position(|cell| {
                cell.power
                    .as_ref()
                    .is_some_and(|p| p.powerset == set && p.internal_name == name)
            })
            .unwrap()
    }

    #[test]
    fn dropping_a_late_power_on_an_early_one_pushes_the_run_down_one_pick() {
        // The screenshot's case, shrunk: a power at the bottom of the ladder is dropped near
        // the top, and every power between the two takes the next pick along rather than the
        // two ends trading places.
        let mut state = populated();
        let (cells, picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        let p2 = cell_of(&cells, "Prim", "P2");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Insert, None),
            None
        );
        apply_shift(&mut state, &cells, fly, p2);

        assert_eq!(level_of(&state, "Pool", "Fly"), cells[p2].level);
        // P2, which held that cell, took the next pick along — not the level Fly left.
        assert_eq!(level_of(&state, "Prim", "P2"), cells[p2 + 1].level);
        // And so did everything between, so nothing collides and no pick is lost.
        assert_eq!(level_of(&state, "Sec", "S2"), cells[p2 + 3].level);
        assert_eq!(level_of(&state, "Pool", "Hover"), cells[p2 + 7].level);
    }

    #[test]
    fn slots_and_enhancements_travel_with_the_power() {
        let mut state = populated();
        let slots = vec![None, None, None];
        state.pools[0].powers[1].slots = slots.clone();
        let (cells, picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        let first = cell_of(&cells, "Prim", "P1");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, first, DropMode::Insert, None),
            None
        );
        apply_shift(&mut state, &cells, fly, first);
        // Floors, slots and toggles ride along with the pick; only the level changed.
        assert_eq!(state.pools[0].powers[1].internal_name, "Fly");
        assert_eq!(state.pools[0].powers[1].slots, slots);
    }

    #[test]
    fn an_empty_cell_target_is_a_plain_move() {
        // An empty cell carries nothing, so dropping on one moves only the dragged pick.
        let mut state = populated();
        let (cells, picks) = grid(&state);
        let empty = cells.iter().position(|cell| cell.power.is_none()).unwrap();
        let fly = cell_of(&cells, "Pool", "Fly");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, empty, DropMode::Insert, None),
            None
        );
        apply_shift(&mut state, &cells, fly, empty);
        assert_eq!(level_of(&state, "Pool", "Fly"), cells[empty].level);
        // Nothing else moved: the pick it passed was empty.
        assert_eq!(level_of(&state, "Prim", "P2"), 2);
    }

    #[test]
    fn a_shift_that_would_pull_a_carried_power_below_its_floor_is_refused() {
        let state = populated();
        let (cells, mut picks) = grid(&state);
        // A late power late in the run, as one carried along by a backwards drag would be.
        let fly = cell_of(&cells, "Pool", "Fly");
        if let Some(pick) = picks[fly].as_mut() {
            pick.floor = Some(cells[fly].level);
        }
        // Dragging Fly back to P2's cell carries P2, S2, P3 and Hover along, and Fly's own
        // floor is above the cell it is dropped on — the reach-back a late power refuses.
        let fly = cell_of(&cells, "Pool", "Fly");
        let p2 = cell_of(&cells, "Prim", "P2");
        let refusal =
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Insert, None).unwrap();
        assert!(refusal.contains("Fly"), "{refusal}");
        assert!(refusal.contains("unlocks at level"), "{refusal}");
    }

    #[test]
    fn an_unresolvable_pick_cannot_be_carried() {
        let state = populated();
        let (cells, mut picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        if let Some(pick) = picks[fly].as_mut() {
            pick.floor = None;
        }
        let p2 = cell_of(&cells, "Prim", "P2");
        let refusal =
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Insert, None).unwrap();
        assert!(refusal.contains("unlock level is unknown"), "{refusal}");
    }

    #[test]
    fn level_one_keeps_one_primary_and_one_secondary() {
        let state = populated();
        let (cells, picks) = grid(&state);
        // Level 1 holds one primary pick and one secondary pick, so a second primary pick
        // arriving there is refused whichever of the two cells it lands in.
        let p1 = cell_of(&cells, "Prim", "P1");
        let p2 = cell_of(&cells, "Prim", "P2");
        let s1 = cell_of(&cells, "Sec", "S1");
        assert!(drop_refusal(&state, &cells, &picks, p2, p1, DropMode::Insert, None).is_some());
        assert!(drop_refusal(&state, &cells, &picks, p2, s1, DropMode::Insert, None).is_some());
        // A secondary pick making the same move keeps the count, so it is allowed.
        let s2 = cell_of(&cells, "Sec", "S2");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, s2, s1, DropMode::Insert, None),
            None
        );
    }

    #[test]
    fn level_up_mode_refuses_a_carried_pick_above_the_character() {
        let state = populated();
        let (cells, picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        let p1 = cell_of(&cells, "Prim", "P1");
        // Dragging the last pick up to the first carries the whole run past a level-3
        // character, so something would land above where they have got to.
        let refusal =
            drop_refusal(&state, &cells, &picks, fly, p1, DropMode::Insert, Some(3)).unwrap();
        assert!(refusal.contains("Level Up mode"), "{refusal}");
        // Within the character's level it is allowed.
        assert_eq!(
            drop_refusal(
                &state,
                &cells,
                &picks,
                fly,
                p1,
                DropMode::Insert,
                Some(cells[fly].level)
            ),
            None
        );
    }

    #[test]
    fn same_cell_is_not_a_move() {
        let state = populated();
        let (cells, picks) = grid(&state);
        let p1 = cell_of(&cells, "Prim", "P1");
        assert!(drop_refusal(&state, &cells, &picks, p1, p1, DropMode::Insert, None).is_some());
    }

    #[test]
    fn a_swap_trades_only_the_two_levels() {
        // The arrangement that was lost when the shift landed: two picks trade places and the
        // run between them is untouched, which is what you want when only the two are wrong.
        let mut state = populated();
        let (cells, picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        let p2 = cell_of(&cells, "Prim", "P2");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Swap, None),
            None
        );
        let between: Vec<u8> = (p2 + 1..fly)
            .filter_map(|i| cells[i].power.as_ref().map(|p| p.level))
            .collect();
        apply_swap(&mut state, &cells, fly, p2);
        assert_eq!(level_of(&state, "Pool", "Fly"), cells[p2].level);
        assert_eq!(level_of(&state, "Prim", "P2"), cells[fly].level);
        // Nothing between the two moved, which is the whole difference from an insert.
        let after: Vec<u8> = (p2 + 1..fly)
            .filter_map(|cell| {
                state
                    .picked_powers()
                    .find(|p| {
                        Some(p.internal_name.as_str())
                            == cells[cell].power.as_ref().map(|c| c.internal_name.as_str())
                    })
                    .map(|p| p.level)
            })
            .collect();
        assert_eq!(after, between);
    }

    #[test]
    fn a_middle_drop_onto_an_unspent_pick_moves_only_the_dragged_power() {
        // An empty cell has no second power to swap with, so dropping on its middle simply
        // moves the dragged pick to that cell's level without shifting powers in between.
        let mut state = populated();
        let (cells, picks) = grid(&state);
        // Cell 3 (level 3) is empty while S2 sits at level 4 and Hover at level 8.
        let empty = cells.iter().position(|cell| cell.power.is_none()).unwrap();
        let fly = cell_of(&cells, "Pool", "Fly");
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, empty, DropMode::Swap, None),
            None
        );
        apply_swap(&mut state, &cells, fly, empty);
        assert_eq!(level_of(&state, "Pool", "Fly"), cells[empty].level);
        assert_eq!(level_of(&state, "Sec", "S2"), 4);
        assert_eq!(level_of(&state, "Pool", "Hover"), 8);
    }

    #[test]
    fn a_swap_is_refused_when_the_occupant_cannot_descend() {
        // A swap moves BOTH ends, so the occupant's floor binds too: dragging an early pick
        // onto a late, gated power would send that power down below where it may be taken.
        let state = populated();
        let (cells, mut picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        if let Some(pick) = picks[fly].as_mut() {
            pick.floor = Some(cells[fly].level);
        }
        let p1 = cell_of(&cells, "Prim", "P1");
        let refusal = drop_refusal(&state, &cells, &picks, p1, fly, DropMode::Swap, None).unwrap();
        assert!(refusal.contains("Fly"), "{refusal}");
    }

    #[test]
    fn the_middle_of_a_card_swaps_and_the_edges_insert_before_or_after() {
        // The two operations share one target and are told apart by where on the card the
        // pointer is: the middle third swaps, top third inserts before, bottom third inserts
        // after. The card is 57px in the built app (measured 2026-11-10), so each third is 19px.
        let (top, bottom) = (100.0, 157.0);
        assert_eq!(CellBand::at(110.0, top, bottom), CellBand::Before);
        assert_eq!(CellBand::at(128.5, top, bottom), CellBand::Middle);
        assert_eq!(CellBand::at(149.0, top, bottom), CellBand::After);
        // Boundaries belong to the outer bands, so aiming at an edge is forgiving.
        assert_eq!(CellBand::at(119.0, top, bottom), CellBand::Before);
        assert_eq!(CellBand::at(138.0, top, bottom), CellBand::After);
        // A zero-height box before layout lands reads as Middle (touches at most two picks).
        assert_eq!(CellBand::at(100.0, top, top), CellBand::Middle);

        // Both sides of the seam between cell 9 (L16) and cell 10 (L18) resolve to the same
        // destination whether dragging backwards (from 22) or forwards (from 2).
        assert_eq!(
            CellBand::After.resolve(22, 9),
            CellBand::Before.resolve(22, 10)
        );
        assert_eq!(CellBand::Before.resolve(22, 10), (10, DropMode::Insert));
        assert_eq!(
            CellBand::After.resolve(2, 9),
            CellBand::Before.resolve(2, 10)
        );
        assert_eq!(CellBand::Before.resolve(2, 10), (9, DropMode::Insert));
        // Inserting after the final cell (23) lands at 23; adjacent seams around `from` stay put.
        assert_eq!(CellBand::After.resolve(2, 23), (23, DropMode::Insert));
        assert_eq!(CellBand::After.resolve(5, 4), (5, DropMode::Insert));
        assert_eq!(CellBand::Before.resolve(5, 6), (5, DropMode::Insert));

        // The 3px vertical gap between stacked cards in a column snaps to the seam between them,
        // while a column wrap (negative gap) does not.
        let drag = PickDrag {
            from_cell: 0,
            origin: (0.0, 0.0),
            moved: true,
            refusals: Vec::new(),
            rects: vec![
                (7, 10.0, 743.0, 260.0, 799.0),
                (8, 280.0, 326.0, 530.0, 382.0),
                (9, 280.0, 385.0, 530.0, 442.0),
            ],
            hover: None,
        };
        assert_eq!(drag.band_at(400.0, 383.5), Some((9, CellBand::Before)));
        assert_eq!(drag.band_at(270.0, 500.0), None);
    }

    #[test]
    fn swap_and_insert_disagree_about_who_moves() {
        // Same drag, same target, two answers: the shape the two zones exist to offer.
        let state = populated();
        let (cells, picks) = grid(&state);
        let fly = cell_of(&cells, "Pool", "Fly");
        let p2 = cell_of(&cells, "Prim", "P2");
        let mut swapped = populated();
        let mut inserted = populated();
        apply_swap(&mut swapped, &cells, fly, p2);
        apply_shift(&mut inserted, &cells, fly, p2);
        assert_ne!(
            level_of(&swapped, "Sec", "S2"),
            level_of(&inserted, "Sec", "S2"),
            "a power between the two keeps its level on a swap and moves on an insert"
        );
        // Both agree about where the dragged pick lands.
        assert_eq!(level_of(&swapped, "Pool", "Fly"), cells[p2].level);
        assert_eq!(level_of(&inserted, "Pool", "Fly"), cells[p2].level);
        // And neither verdict refused the drop on the facts the press measured.
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Swap, None),
            None
        );
        assert_eq!(
            drop_refusal(&state, &cells, &picks, fly, p2, DropMode::Insert, None),
            None
        );
    }

    #[test]
    fn a_shift_reorders_the_cells_and_leaves_none_behind() {
        // The pure shape of the move: a remove-then-insert over the cells, so every cell has
        // exactly one pick and the two ends are the only ones that changed place by design.
        assert_eq!(shift_sources(5, 4, 1), vec![0, 4, 1, 2, 3]);
        assert_eq!(shift_sources(5, 1, 4), vec![0, 2, 3, 4, 1]);
        assert_eq!(shift_sources(5, 2, 2), vec![0, 1, 2, 3, 4]);
        // Everything carried is what moved, and the two ends are not among it.
        assert_eq!(carried_cells(5, 4, 1), vec![2, 3]);
        assert_eq!(carried_cells(5, 1, 4), vec![2, 3]);
        assert_eq!(carried_cells(5, 0, 1), Vec::<usize>::new());
    }
}
