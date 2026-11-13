//! The phone's Build tab (phone-build-flow, phone-slot-cards): one list of the build's powers in
//! level order. Each chosen power is a card whose slots show as circles in the game's colours,
//! and whose Slots button unfolds them beneath it to fill, add or remove.
//!
//! Nothing here decides what a build may hold. Choosing a power borrows the Available panel's
//! own rows inside a sheet ([`PickSheet`]); every rule about what a level may take lives in
//! those rows, and slot limits are the desktop card's own ([`slot_room`]).

use crate::build_session::BuildSession;
use crate::level_control::TargetSlot;
use crate::mobile_nav::{OpenSheet, Sheet};
use crate::panels::powers::{
    doomed_slot_range, slot_room, BuildBudget, PickSheetLevel, PickerDestination, PickerOpen,
    PickerTarget, SlotDrag, SlotHandle,
};
use crate::panels::powers_by_level::{deal_picks, PickCell};
use crate::shell::Db;
use crate::view::power_view::resolve_power_def;
use coh_data::InherentCategory;
use dioxus::prelude::*;

/// The phone's two main views. Build is the list of powers; Stats is every panel the phone
/// shows, the stat panels first, in the user's own phone order (Display > Reorder). More is a
/// sheet over either, so it is not one of these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Build,
    Stats,
}

/// Which main view the phone shows, shared by the bottom bar, the key numbers and the shell.
#[derive(Clone, Copy)]
pub struct PhoneTab(pub Signal<Tab>);

/// Which card has its slots unfolded, by the power's address. One at a time.
#[derive(Clone, Copy)]
pub struct OpenCard(pub Signal<Option<String>>);

/// The power the phone's details sheet shows ([`PowerInfoSheet`]); `None` while it is closed.
#[derive(Clone, Copy)]
pub struct PhoneInfo(pub Signal<crate::shell::Selection>);

pub fn use_phone_job_provider() {
    let tab = use_signal(|| Tab::Build);
    use_context_provider(|| PhoneTab(tab));
    let open = use_signal(|| None);
    use_context_provider(|| OpenCard(open));
    let info = use_signal(|| None);
    use_context_provider(|| PhoneInfo(info));
}

/// A phone has no hover to read a power by, so this small button reads it: it opens the details
/// sheet on its power. Beside a power's name on the Build tab and on each level-sheet row.
#[component]
pub fn InfoButton(target: crate::shell::PowerRef, name: String) -> Element {
    let mut info = use_context::<PhoneInfo>().0;
    rsx! {
        button {
            class: "power-info-button",
            r#type: "button",
            "aria-label": "About {name}",
            title: "About {name}",
            onclick: move |evt| {
                evt.stop_propagation();
                info.set(Some(target.clone()));
            },
            svg {
                view_box: "0 0 16 16",
                width: "16",
                height: "16",
                "aria-hidden": "true",
                fill: "none",
                stroke: "currentColor",
                stroke_width: "1.5",
                stroke_linecap: "round",
                circle { cx: "8", cy: "8", r: "6.5" }
                path { d: "M8 7.25v4" }
                circle { cx: "8", cy: "4.9", r: "0.4", fill: "currentColor" }
            }
        }
    }
}

/// The details sheet: the desktop Info panel's own view of one power, held fixed, in a modal so
/// it opens over the level sheet and closing it goes back there. Hosted at the shell root.
#[component]
pub fn PowerInfoSheet(database: Db) -> Element {
    let mut info = use_context::<PhoneInfo>().0;
    let Some(target) = info() else {
        return rsx! {};
    };
    let title = crate::naming::power_label(Some(&database), &target.powerset_id, &target.power);
    rsx! {
        crate::modal::Modal {
            title,
            size: crate::modal::ModalSize::Full,
            on_close: move |_| info.set(None),
            div { class: "power-info-sheet",
                crate::panels::info::Info { database, selection: info, fixed: true }
            }
        }
    }
}

/// The build's picks dealt onto the schedule's pick levels, up to the build's own level, and any
/// pick that fits no cell (a build carrying levels this schedule never grants).
fn cells_of(database: &Db, build: &coh_data::CharacterState) -> Option<(Vec<PickCell>, usize)> {
    let schedule = database.leveling_schedule.as_ref()?;
    let levels: Vec<u8> = schedule
        .pick_levels()
        .into_iter()
        .filter(|&level| level <= build.level)
        .collect();
    let (cells, unplaced) = deal_picks(&levels, build.picked_powers().cloned().collect());
    Some((cells, unplaced.len()))
}

/// The first empty pick at or after `from`, wrapping round to the start — the next level the
/// list asks for.
fn next_empty(cells: &[PickCell], from: u8) -> Option<u8> {
    cells
        .iter()
        .find(|cell| cell.power.is_none() && cell.level >= from)
        .or_else(|| cells.iter().find(|cell| cell.power.is_none()))
        .map(|cell| cell.level)
}

/// A pick's set, by name: a powerset's own, or its pool's.
fn set_label(database: &Db, set_id: &str) -> String {
    database
        .find_powerset(set_id)
        .map(|set| set.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::naming::pool_name(set_id, database))
}

/// The granted inherents that take enhancements (Health, Stamina, Sprint and the like), which
/// players slot too. Grants that take none (the archetype inherents) have no card.
fn slotted_inherents<'a>(
    database: &Db,
    build: &'a coh_data::CharacterState,
) -> Vec<&'a coh_data::SelectedPower> {
    build
        .inherents
        .iter()
        .filter(|power| {
            resolve_power_def(database, &power.powerset, &power.internal_name)
                .is_some_and(|def| def.accepts_enhancements())
        })
        .collect()
}

/// The counts the old pass switch carried, as one line in the build bar: picks made, extra
/// slots placed, slots filled, each against what the build's level allows.
#[component]
pub fn BuildCounts(database: Option<Db>) -> Element {
    if use_context::<PhoneTab>().0() == Tab::Stats {
        return rsx! {};
    }
    let session = use_context::<BuildSession>();
    let slots = use_context::<BuildBudget>().0();
    let (picks, filled) = {
        let build = session.build.read();
        let pick_budget = database
            .as_ref()
            .and_then(|db| db.leveling_schedule.as_ref())
            .map(|schedule| schedule.total_power_picks_at_level(build.level));
        let mut all: Vec<&coh_data::SelectedPower> = build.picked_powers().collect();
        if let Some(db) = database.as_ref() {
            all.extend(slotted_inherents(db, &build));
        }
        let (full, total) = all.iter().fold((0, 0), |(full, total), power| {
            (
                full + power.slots.iter().filter(|slot| slot.is_some()).count(),
                total + power.slots.len(),
            )
        });
        (
            pick_budget.map(|budget| (build.picked_powers().count(), budget)),
            (full, total),
        )
    };
    let entries = [
        ("Powers", picks),
        ("Slots", Some((slots.used, slots.available))),
        ("Filled", Some(filled)),
    ];
    rsx! {
        div { class: "build-counts",
            for (label, count) in entries {
                span {
                    key: "{label}",
                    class: match count {
                        Some((used, available)) if used > available => "build-counts__count is-over",
                        _ => "build-counts__count",
                    },
                    span { class: "build-counts__label", "{label}" }
                    match count {
                        Some((used, available)) => rsx! { b { class: "mono", "{used}/{available}" } },
                        None => rsx! { b { "—" } },
                    }
                }
            }
        }
    }
}

/// One slot as a folded card draws it.
#[derive(Clone, PartialEq)]
enum Dot {
    Empty,
    /// Slotted, in its in-game colour ([`crate::slot_colour`]).
    Filled(crate::slot_colour::SlotColour),
}

/// What a card shows of one power. Built once per render from the build, then handed to
/// [`PowerCard`] whole.
#[derive(Clone, PartialEq)]
struct CardData {
    key: String,
    set_id: String,
    ident: String,
    name: String,
    set: String,
    dots: Vec<Dot>,
    icons: Vec<Option<String>>,
    max_add: usize,
    max_remove: usize,
    /// The trailing slots an inherent grants, which a drag left never takes.
    inherent_slots: usize,
}

fn card_data(
    database: &Db,
    power: &coh_data::SelectedPower,
    budget: Option<usize>,
    used: usize,
) -> CardData {
    let def = resolve_power_def(database, &power.powerset, &power.internal_name);
    let (max_add, max_remove) = slot_room(def, power, budget, used);
    CardData {
        key: power.address(),
        set_id: power.powerset.clone(),
        ident: power.internal_name.clone(),
        name: crate::naming::power_label(Some(database), &power.powerset, &power.internal_name),
        set: set_label(database, &power.powerset),
        dots: power
            .slots
            .iter()
            .map(|slot| match slot {
                None => Dot::Empty,
                Some(enhancement) => {
                    Dot::Filled(crate::slot_colour::slot_colour(enhancement, database))
                }
            })
            .collect(),
        icons: power
            .slots
            .iter()
            .map(|slot| {
                slot.as_ref().map(|enhancement| {
                    crate::panels::powers::slot_icon_url(enhancement, database).unwrap_or_default()
                })
            })
            .collect(),
        max_add,
        max_remove,
        inherent_slots: power.inherent_slot_count as usize,
    }
}

fn open_picker(
    mut picker: Signal<Option<PickerTarget>>,
    set_id: String,
    ident: String,
    slot_index: u8,
) {
    picker.set(Some(PickerTarget {
        powerset_id: set_id,
        power_internal_name: ident,
        slot_index,
        destination: PickerDestination::Build,
    }));
}

/// The Build tab: every pick level in order, each holding its power's card or asking for one,
/// then the slotted inherents, and a button at the foot for the next thing to do.
#[component]
pub fn BuildList(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let mut sheet = use_context::<OpenSheet>().0;
    let mut open = use_context::<OpenCard>().0;
    let picker = use_context::<PickerOpen>().0;
    let mut folds = use_context::<crate::inherent_fold_store::InherentFolds>().0;
    let build = session.build.read();
    if build.archetype.id.is_none() {
        return rsx! {
            div { class: "phone-job",
                p { class: "phone-job__note", "Start by choosing your archetype and powersets." }
                button {
                    class: "phone-job__cta",
                    r#type: "button",
                    onclick: move |_| sheet.set(Some(Sheet::Identity)),
                    "Choose an archetype"
                }
            }
        };
    }
    let Some((cells, unplaced)) = cells_of(&database, &build) else {
        return rsx! {
            div { class: "load-state error",
                "This dataset carries no leveling schedule, so the levels powers are picked at are unknown."
            }
        };
    };
    let budget = database
        .leveling_schedule
        .as_ref()
        .map(|schedule| schedule.total_slots_at_level(build.level));
    let used = coh_data::placed_budget_slots(&build);
    let left = budget.map(|budget| budget.saturating_sub(used));
    let next = next_empty(&cells, 0);
    let rows: Vec<(u8, Option<CardData>)> = cells
        .iter()
        .map(|cell| {
            let card = cell
                .power
                .as_ref()
                .map(|power| card_data(&database, power, budget, used));
            (cell.level, card)
        })
        .collect();
    // The inherents in the desktop's five groups and order; a group with no card is left out.
    let slotted = slotted_inherents(&database, &build);
    let groups: Vec<(InherentCategory, Vec<CardData>)> = crate::grid::model::INHERENT_GROUPS
        .into_iter()
        .map(|group| {
            let cards = slotted
                .iter()
                .filter(|power| power.inherent_category == Some(group))
                .map(|power| card_data(&database, power, budget, used))
                .collect::<Vec<_>>();
            (group, cards)
        })
        .filter(|(_, cards)| !cards.is_empty())
        .collect();
    drop(build);

    // With every level chosen, the foot leads to the first power with an empty slot, and
    // opens its inherent group first if that is folded.
    let next_slot = rows
        .iter()
        .filter_map(|(_, card)| card.as_ref().map(|card| (None, card)))
        .chain(
            groups
                .iter()
                .flat_map(|(group, cards)| cards.iter().map(|card| (Some(*group), card))),
        )
        .find_map(|(group, card)| {
            card.dots
                .iter()
                .position(|dot| *dot == Dot::Empty)
                .map(|at| {
                    (
                        group,
                        card.key.clone(),
                        card.set_id.clone(),
                        card.ident.clone(),
                        card.name.clone(),
                        at as u8,
                    )
                })
        });

    rsx! {
        div { class: "phone-job",
            p { class: "phone-job__note",
                "Tap an empty level to choose its power, or a power to change it. Slots opens a power's slots."
            }
            if unplaced > 0 {
                p { class: "phone-job__fault",
                    "{unplaced} chosen powers fit no level this server grants. Remove them from the Stats tab's Powers panel."
                }
            }
            ol { class: "phone-picks",
                // Keyed by position as well as power: two levels can share a number (the two
                // level-1 picks), and two empty ones share everything else.
                for (index, (level, card)) in rows.into_iter().enumerate() {
                    li { key: "{index}-{card.as_ref().map(|c| c.key.as_str()).unwrap_or(\"\")}",
                        match card {
                            Some(card) => rsx! {
                                PowerCard { database: database.clone(), card, level: Some(level), left, budget: budget.unwrap_or(0) }
                            },
                            None => rsx! {
                                button {
                                    class: if Some(level) == next { "phone-pick is-empty is-next" } else { "phone-pick is-empty" },
                                    r#type: "button",
                                    onclick: move |_| sheet.set(Some(Sheet::Pick(level))),
                                    span { class: "phone-pick__level mono", "{level}" }
                                    span { class: "phone-pick__body",
                                        span { class: "phone-pick__name", "Choose a power" }
                                    }
                                    if Some(level) == next {
                                        span { class: "phone-pick__next", "Next" }
                                    }
                                }
                            },
                        }
                    }
                }
            }
            if !groups.is_empty() {
                h3 { class: "phone-picks__heading", "Inherent powers" }
                for (group, cards) in groups {
                    InherentGroup {
                        key: "{crate::grid::model::inherent_group_slug(group)}",
                        database: database.clone(),
                        group,
                        cards,
                        left,
                        budget: budget.unwrap_or(0),
                    }
                }
            }
            if let Some(level) = next {
                div { class: "phone-job__foot",
                    button {
                        class: "phone-job__cta",
                        r#type: "button",
                        onclick: move |_| sheet.set(Some(Sheet::Pick(level))),
                        "Choose your level {level} power"
                    }
                }
            } else if let Some((group, key, set_id, ident, name, at)) = next_slot {
                div { class: "phone-job__foot",
                    button {
                        class: "phone-job__cta",
                        r#type: "button",
                        onclick: move |_| {
                            if let Some(group) = group {
                                folds.write().insert(group, false);
                            }
                            open.set(Some(key.clone()));
                            open_picker(picker, set_id.clone(), ident.clone(), at);
                        },
                        "Fill the next empty slot: {name}"
                    }
                }
            }
        }
    }
}

/// One inherent group under a heading that folds it open or shut. The fold is the desktop
/// panels' own memory ([`crate::inherent_fold_store`]), so both layouts agree.
#[component]
fn InherentGroup(
    database: Db,
    group: InherentCategory,
    cards: Vec<CardData>,
    left: Option<usize>,
    budget: usize,
) -> Element {
    let folds = use_context::<crate::inherent_fold_store::InherentFolds>().0;
    let fold = crate::inherent_fold_store::InherentFold::new(folds, group);
    let folded = fold.folded();
    let title = crate::grid::model::inherent_group_title(group);
    let slots: usize = cards.iter().map(|card| card.dots.len()).sum();
    let filled: usize = cards
        .iter()
        .flat_map(|card| card.dots.iter())
        .filter(|dot| **dot != Dot::Empty)
        .count();
    let powers = match cards.len() {
        1 => "1 power".to_string(),
        n => format!("{n} powers"),
    };
    rsx! {
        section { class: "phone-group",
            button {
                class: "phone-group__head",
                r#type: "button",
                aria_expanded: "{!folded}",
                onclick: move |_| fold.toggle(),
                span { class: "phone-group__title", "{title}" }
                span { class: "phone-group__count", "{powers} · {filled}/{slots} filled" }
                span { class: "phone-group__chevron", if folded { "▸" } else { "▾" } }
            }
            if !folded {
                ol { class: "phone-picks",
                    for card in cards {
                        li { key: "{card.key}",
                            PowerCard { database: database.clone(), card, level: None, left, budget }
                        }
                    }
                }
            }
        }
    }
}

/// One chosen power: its name (tap to change it, for a pick), its slots as circles, and a Slots
/// button that unfolds them beneath the card — each slot opens the enhancement picker on that
/// slot, and a minus and plus button take a slot away or add one.
#[component]
fn PowerCard(
    database: Db,
    card: CardData,
    level: Option<u8>,
    left: Option<usize>,
    /// The build's whole slot budget at its level, which the drag's adds are checked against.
    budget: usize,
) -> Element {
    let mut sheet = use_context::<OpenSheet>().0;
    let mut open = use_context::<OpenCard>().0;
    let level_up = *use_context::<crate::level_control::LevelUpMode>().0.read();
    let picker = use_context::<PickerOpen>().0;
    let is_open = open().as_deref() == Some(card.key.as_str());
    let filled = card.dots.iter().filter(|dot| **dot != Dot::Empty).count();
    let count = card.dots.len();
    let CardData {
        key,
        set_id,
        ident,
        name,
        set,
        dots,
        icons,
        max_add,
        max_remove,
        inherent_slots,
    } = card;
    // The slot drag in flight on this card, shared with its handle so the cells can preview it:
    // ghost slots for a drag right, the leaving slots marked for a drag left.
    let drag = use_signal(|| Option::<SlotDrag>::None);
    let pending = SlotDrag::pending(drag());
    let doomed = doomed_slot_range(count, inherent_slots, pending);
    let capsule_key = format!("{set_id}/{ident}");

    rsx! {
        div { class: if is_open { "phone-card is-open" } else { "phone-card" },
            div { class: "phone-card__row",
                div { class: "phone-card__name",
                match level {
                    Some(level) => rsx! {
                        button {
                            class: "phone-card__head",
                            r#type: "button",
                            "aria-label": "Change your level {level} power, {name}",
                            onclick: move |_| sheet.set(Some(Sheet::Pick(level))),
                            span { class: "phone-pick__level mono", "{level}" }
                            span { class: "phone-pick__body",
                                span { class: "phone-pick__name", "{name}" }
                                span { class: "phone-pick__set", "{set}" }
                            }
                        }
                    },
                    None => rsx! {
                        div { class: "phone-card__head",
                            span { class: "phone-pick__level mono", "·" }
                            span { class: "phone-pick__body",
                                span { class: "phone-pick__name", "{name}" }
                                span { class: "phone-pick__set", "{set}" }
                            }
                        }
                    },
                }
                InfoButton {
                    target: crate::shell::PowerRef { powerset_id: set_id.clone(), power: ident.clone() },
                    name: name.clone(),
                }
                }
                button {
                    class: "phone-card__toggle",
                    r#type: "button",
                    "aria-expanded": is_open,
                    "aria-label": "Slots for {name}: {filled} of {count} filled",
                    onclick: {
                        let key = key.clone();
                        move |_| open.set((!is_open).then(|| key.clone()))
                    },
                    span { class: "phone-dots", "aria-hidden": "true",
                        for (index, dot) in dots.iter().enumerate() {
                            match dot {
                                Dot::Empty => rsx! { span { key: "{index}", class: "phone-dot" } },
                                Dot::Filled(colour) => rsx! {
                                    span {
                                        key: "{index}",
                                        class: if colour.superior { "phone-dot is-filled is-superior" } else { "phone-dot is-filled" },
                                        style: colour.fill.map(|hex| format!("--dot: {hex}")).unwrap_or_default(),
                                    }
                                },
                            }
                        }
                    }
                    span { class: "phone-card__toggle-label",
                        "Slots"
                        span { class: if is_open { "phone-card__caret is-open" } else { "phone-card__caret" }, "▾" }
                    }
                }
            }
            if is_open {
                div { class: "phone-card__slots",
                    div {
                        class: if drag().is_some() { "phone-slots is-active" } else { "phone-slots" },
                        "data-slot-capsule": "{capsule_key}",
                        div { class: "phone-slots__cells",
                            for (index, icon) in icons.iter().cloned().enumerate() {
                                button {
                                    key: "{index}",
                                    class: match (icon.is_some(), doomed.contains(&index)) {
                                        (_, true) => "phone-enh is-doomed",
                                        (true, false) => "phone-enh",
                                        (false, false) => "phone-enh is-empty",
                                    },
                                    r#type: "button",
                                    "aria-label": if icon.is_some() { "Slot {index + 1} of {name}" } else { "Empty slot {index + 1} of {name}" },
                                    onclick: {
                                        let (set_id, ident) = (set_id.clone(), ident.clone());
                                        move |_| open_picker(picker, set_id.clone(), ident.clone(), index as u8)
                                    },
                                    if let Some(url) = icon.as_deref().filter(|url| !url.is_empty()) {
                                        img { class: "phone-enh__art", src: "{url}", alt: "" }
                                    }
                                }
                            }
                            for ghost in 0..pending.max(0) {
                                span { key: "ghost-{ghost}", class: "phone-enh is-ghost" }
                            }
                        }
                        if max_add >= 1 || max_remove >= 1 {
                            SlotHandle {
                                drag,
                                capsule_key: capsule_key.clone(),
                                max_add,
                                max_remove,
                                database: database.clone(),
                                powerset_id: set_id.clone(),
                                power_id: ident.clone(),
                                slot_budget: budget,
                                leveling_on: level_up,
                            }
                        }
                    }
                    div { class: "phone-card__room",
                        span { class: "phone-card__hint", "Drag ‹› right to add slots, left to remove" }
                        if let Some(left) = left {
                            span { class: "phone-card__left",
                                b { class: "mono", "{left}" }
                                " left"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The sheet behind a level: the Available panel's powerset lists and the Pools panel, both
/// told which level they are choosing for. Whenever the level goes from having room to full —
/// a pick into an empty level, or a change (remove, then choose again) — the sheet moves on to
/// the next empty level, or closes when there is none.
#[component]
pub fn PickSheet(level: u8, database: Db) -> Element {
    use_context_provider(|| PickSheetLevel(level));
    let session = use_context::<BuildSession>();
    let mut sheet = use_context::<OpenSheet>().0;
    let mut target = use_context::<TargetSlot>().0;

    // Whether the level had room at the last look; filling it is the change from true to false.
    let mut had_room = use_signal(|| false);
    use_effect(move || {
        target.set(Some(level));
    });
    // Closing the sheet un-steers: the desktop grid reads the same marker.
    use_drop(move || TargetSlot(target).clear());
    use_effect({
        let database = database.clone();
        move || {
            let build = session.build.read();
            let room = cells_of(&database, &build).is_some_and(|(cells, _)| {
                cells
                    .iter()
                    .any(|cell| cell.level == level && cell.power.is_none())
            });
            let filled = *had_room.peek() && !room;
            had_room.set(room);
            if filled {
                let next =
                    cells_of(&database, &build).and_then(|(cells, _)| next_empty(&cells, level));
                drop(build);
                sheet.set(next.map(Sheet::Pick));
            }
        }
    });

    rsx! {
        div { class: "pick-sheet",
            crate::panels::powers::AvailablePanel { database: database.clone(), picks_only: true }
            h3 { class: "popover-inline__title pick-sheet__pools", "Power pools" }
            crate::panels::powers::PoolsPanel { database }
        }
    }
}

/// The Build Identity sheet's way on: once an archetype and both powersets are chosen on a build
/// with no powers yet, one button goes straight to the first level's pick sheet — a first visit
/// should not have to close the form and find the Powers list to take its first step.
#[component]
pub fn StartPicking(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let mut sheet = use_context::<OpenSheet>().0;
    let mut tab = use_context::<PhoneTab>().0;
    let first = {
        let build = session.build.read();
        let ready = build.archetype.id.is_some()
            && build.primary.id.is_some()
            && build.secondary.id.is_some()
            && build.picked_powers().next().is_none();
        ready
            .then(|| cells_of(&database, &build).and_then(|(cells, _)| next_empty(&cells, 0)))
            .flatten()
    };
    let Some(level) = first else {
        return rsx! {};
    };
    rsx! {
        div { class: "start-picking",
            button {
                class: "phone-job__cta",
                r#type: "button",
                onclick: move |_| {
                    tab.set(Tab::Build);
                    sheet.set(Some(Sheet::Pick(level)));
                },
                "Choose your level {level} power"
            }
        }
    }
}
