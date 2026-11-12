//! The phone's Build tab (phone-build-flow): a build made in three passes — choose powers, place
//! slots, fill them — each its own short list of the same picks, picked from a switch in the
//! build bar. The counts on the switch are the bar's old budget counters, so it replaced them.
//!
//! Nothing here decides what a build may hold. Choosing a power borrows the Available panel's
//! own rows inside a sheet ([`PickSheet`]); every rule about what a level may take lives in
//! those rows, so a fix there is a fix here.

use crate::build_session::BuildSession;
use crate::level_control::TargetSlot;
use crate::mobile_nav::{OpenSheet, Sheet};
use crate::panels::powers::{
    add_slot_to_power, remove_slot_from_power, slot_room, BuildBudget, PickSheetLevel,
    PickerDestination, PickerOpen, PickerTarget,
};
use crate::panels::powers_by_level::{deal_picks, PickCell};
use crate::shell::Db;
use crate::view::power_view::resolve_power_def;
use dioxus::prelude::*;

/// The three passes. Powers first, since nothing else can start until a power is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Job {
    Powers,
    Slots,
    Enhance,
}

/// Which pass the Build tab shows, shared by the switch in the bar and the stack below it.
#[derive(Clone, Copy)]
pub struct PhoneJob(pub Signal<Job>);

/// The phone's two main views. Build is the three passes; Stats is every panel the phone
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

pub fn use_phone_job_provider() {
    let job = use_signal(|| Job::Powers);
    use_context_provider(|| PhoneJob(job));
    let tab = use_signal(|| Tab::Build);
    use_context_provider(|| PhoneTab(tab));
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
/// Powers pass asks for.
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

/// The powers the Slots and Enhance passes list: the picks in level order, then the granted
/// inherents that take enhancements (Health, Stamina, Sprint and the like), which players slot
/// too. Grants that take none (the archetype inherents) have nothing to do on either pass.
pub(crate) fn slotting_powers<'a>(
    database: &Db,
    build: &'a coh_data::CharacterState,
) -> Vec<&'a coh_data::SelectedPower> {
    let mut picks: Vec<&coh_data::SelectedPower> = build.picked_powers().collect();
    picks.sort_by_key(|power| power.level);
    let inherents = build.inherents.iter().filter(|power| {
        resolve_power_def(database, &power.powerset, &power.internal_name)
            .is_some_and(|def| def.accepts_enhancements())
    });
    picks.into_iter().chain(inherents).collect()
}

/// The switch: three buttons, each with how far that pass has got.
#[component]
pub fn JobSwitch(database: Option<Db>) -> Element {
    let mut job = use_context::<PhoneJob>().0;
    // The passes are the Build tab's; on Stats the switch would steer nothing on screen.
    if use_context::<PhoneTab>().0() == Tab::Stats {
        return rsx! {};
    }
    let session = use_context::<BuildSession>();
    let slot_budget = use_context::<BuildBudget>().0;
    let (powers, enhance) = {
        let build = session.build.read();
        let pick_budget = database
            .as_ref()
            .and_then(|db| db.leveling_schedule.as_ref())
            .map(|schedule| schedule.total_power_picks_at_level(build.level));
        let picks = build.picked_powers().count();
        let listed = database
            .as_ref()
            .map(|db| slotting_powers(db, &build))
            .unwrap_or_default();
        let (filled, total) = listed.iter().fold((0, 0), |(filled, total), power| {
            (
                filled + power.slots.iter().filter(|slot| slot.is_some()).count(),
                total + power.slots.len(),
            )
        });
        (pick_budget.map(|budget| (picks, budget)), (filled, total))
    };
    let slots = slot_budget();
    let entries = [
        (Job::Powers, "Powers", powers),
        (Job::Slots, "Slots", Some((slots.used, slots.available))),
        (Job::Enhance, "Enhance", Some(enhance)),
    ];
    rsx! {
        div { class: "job-switch", role: "group", "aria-label": "What you are doing",
            for (which, label, count) in entries {
                button {
                    key: "{label}",
                    class: match count {
                        Some((used, available)) if used > available => "job-switch__job is-over",
                        _ => "job-switch__job",
                    },
                    r#type: "button",
                    "aria-pressed": job() == which,
                    onclick: move |_| job.set(which),
                    span { class: "job-switch__label", "{label}" }
                    span { class: "job-switch__count mono",
                        match count {
                            Some((used, available)) => rsx! { "{used} of {available}" },
                            None => rsx! { "—" },
                        }
                    }
                }
            }
        }
    }
}

/// The Powers pass: every pick level in order, each holding its power or asking for one, and a
/// button at the foot for the next empty level.
#[component]
pub fn PowersJob(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let mut sheet = use_context::<OpenSheet>().0;
    let build = session.build.read();
    if build.archetype.id.is_none() {
        return rsx! {
            div { class: "phone-job",
                p { class: "phone-job__note", "Choose an archetype to start: tap the bar at the top." }
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
    let next = next_empty(&cells, 0);
    type Row = (usize, u8, &'static str, Option<(String, String, usize)>);
    let rows: Vec<Row> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let power = cell.power.as_ref().map(|power| {
                (
                    crate::naming::power_label(
                        Some(&database),
                        &power.powerset,
                        &power.internal_name,
                    ),
                    set_label(&database, &power.powerset),
                    power.slots.len(),
                )
            });
            let class = match (&power, Some(cell.level) == next) {
                (Some(_), _) => "phone-pick",
                (None, true) => "phone-pick is-empty is-next",
                (None, false) => "phone-pick is-empty",
            };
            (index, cell.level, class, power)
        })
        .collect();
    drop(build);

    rsx! {
        div { class: "phone-job",
            p { class: "phone-job__note",
                "Your power choices, in level order. Tap an empty level to fill it, or a power to change it."
            }
            if unplaced > 0 {
                p { class: "phone-job__fault",
                    "{unplaced} chosen powers fit no level this server grants. Remove them from the Stats tab's Powers panel."
                }
            }
            ol { class: "phone-picks",
                for (index, level, class, power) in rows {
                    li { key: "{index}",
                        button {
                            class,
                            r#type: "button",
                            onclick: move |_| sheet.set(Some(Sheet::Pick(level))),
                            span { class: "phone-pick__level mono", "{level}" }
                            match power {
                                Some((name, set, slots)) => rsx! {
                                    span { class: "phone-pick__body",
                                        span { class: "phone-pick__name", "{name}" }
                                        span { class: "phone-pick__set", "{set}" }
                                    }
                                    span { class: "phone-pick__slots", "aria-label": "{slots} slots",
                                        for dot in 0..slots {
                                            span { key: "{dot}", class: "phone-pick__dot" }
                                        }
                                    }
                                },
                                None => rsx! {
                                    span { class: "phone-pick__body",
                                        span { class: "phone-pick__name", "Choose a power" }
                                    }
                                    if Some(level) == next {
                                        span { class: "phone-pick__next", "Next" }
                                    }
                                },
                            }
                        }
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

/// The Slots pass: each power with its slot count and a minus and plus button, under how many
/// slots are left to place. The buttons' limits are the power card's own ([`slot_room`]).
#[component]
pub fn SlotsJob(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let level_up = *use_context::<crate::level_control::LevelUpMode>().0.read();
    let build = session.build.read();
    let budget = database
        .leveling_schedule
        .as_ref()
        .map(|schedule| schedule.total_slots_at_level(build.level));
    let used = coh_data::placed_budget_slots(&build);
    struct Row {
        key: String,
        set_id: String,
        ident: String,
        level: Option<u8>,
        name: String,
        slots: usize,
        room: usize,
        add: bool,
        remove: bool,
    }
    let rows: Vec<Row> = slotting_powers(&database, &build)
        .into_iter()
        .map(|power| {
            let def = resolve_power_def(&database, &power.powerset, &power.internal_name);
            let (max_add, max_remove) = slot_room(def, power, budget, used);
            // The power's whole room, as if the budget were endless, so a spent budget still
            // shows how far each power could go.
            let (unbounded, _) = slot_room(def, power, Some(usize::MAX), 0);
            Row {
                key: power.address(),
                set_id: power.powerset.clone(),
                ident: power.internal_name.clone(),
                level: (!power.is_locked && power.level > 0).then_some(power.level),
                name: crate::naming::power_label(
                    Some(&database),
                    &power.powerset,
                    &power.internal_name,
                ),
                slots: power.slots.len(),
                room: power.slots.len() + unbounded,
                add: max_add > 0,
                remove: max_remove > 0,
            }
        })
        .collect();
    let left = budget.map(|budget| budget.saturating_sub(used));
    let empty = build.picked_powers().next().is_none();
    drop(build);

    rsx! {
        div { class: "phone-job",
            p { class: "phone-job__note",
                match left {
                    Some(left) => rsx! {
                        b { class: "mono", "{left}" }
                        " extra slots left to place. Each power starts with one, and most take up to six."
                    },
                    None => rsx! { "This dataset carries no leveling schedule, so the slot budget is unknown." },
                }
            }
            if empty {
                p { class: "phone-job__note", "Choose some powers first." }
            }
            ol { class: "phone-picks",
                for row in rows {
                    li { key: "{row.key}",
                        div { class: "phone-pick phone-pick--slots",
                            span { class: "phone-pick__level mono",
                                if let Some(level) = row.level { "{level}" } else { "·" }
                            }
                            span { class: "phone-pick__body",
                                span { class: "phone-pick__name", "{row.name}" }
                                span { class: "phone-pick__slots", "aria-hidden": "true",
                                    for dot in 0..row.room {
                                        span {
                                            key: "{dot}",
                                            class: if dot < row.slots { "phone-pick__dot" } else { "phone-pick__dot is-off" },
                                        }
                                    }
                                }
                            }
                            span { class: "phone-stepper",
                                button {
                                    class: "phone-stepper__step",
                                    r#type: "button",
                                    disabled: !row.remove,
                                    "aria-label": "Remove a slot from {row.name}",
                                    onclick: {
                                        let (set_id, ident, database) = (row.set_id.clone(), row.ident.clone(), database.clone());
                                        move |_| {
                                            let (set_id, ident, database) = (set_id.clone(), ident.clone(), database.clone());
                                            session.commit(move |state| {
                                                remove_slot_from_power(state, &set_id, &ident, database.leveling_schedule.as_ref());
                                            });
                                        }
                                    },
                                    "−"
                                }
                                span { class: "phone-stepper__count mono", "{row.slots}" }
                                button {
                                    class: "phone-stepper__step",
                                    r#type: "button",
                                    disabled: !row.add,
                                    "aria-label": "Add a slot to {row.name}",
                                    onclick: {
                                        let (set_id, ident, database) = (row.set_id.clone(), row.ident.clone(), database.clone());
                                        move |_| {
                                            let (set_id, ident, database) = (set_id.clone(), ident.clone(), database.clone());
                                            session.commit(move |state| {
                                                let leveling = database.leveling_schedule.as_ref();
                                                let budget = leveling.map_or(0, |schedule| schedule.total_slots_at_level(state.level));
                                                add_slot_to_power(state, &set_id, &ident, budget, leveling, level_up);
                                            });
                                        }
                                    },
                                    "+"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The Enhance pass: each power with its slots drawn small, and how many are empty. Tapping a
/// power opens the enhancement picker on its first empty slot (the picker fills on from there);
/// tapping one slot opens that slot. The picker is the desktop's own, so every kind of
/// enhancement it offers is offered here.
#[component]
pub fn EnhanceJob(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let picker = use_context::<PickerOpen>().0;
    let build = session.build.read();
    struct Row {
        key: String,
        set_id: String,
        ident: String,
        level: Option<u8>,
        name: String,
        slots: Vec<Option<String>>,
        empty: usize,
    }
    let rows: Vec<Row> = slotting_powers(&database, &build)
        .into_iter()
        .map(|power| Row {
            key: power.address(),
            set_id: power.powerset.clone(),
            ident: power.internal_name.clone(),
            level: (!power.is_locked && power.level > 0).then_some(power.level),
            name: crate::naming::power_label(
                Some(&database),
                &power.powerset,
                &power.internal_name,
            ),
            slots: power
                .slots
                .iter()
                .map(|slot| {
                    slot.as_ref().map(|enhancement| {
                        crate::panels::powers::slot_icon_url(enhancement, &database)
                            .unwrap_or_default()
                    })
                })
                .collect(),
            empty: power.slots.iter().filter(|slot| slot.is_none()).count(),
        })
        .collect();
    let empty_build = build.picked_powers().next().is_none();
    drop(build);
    let first_empty =
        |slots: &[Option<String>]| slots.iter().position(Option::is_none).unwrap_or(0) as u8;
    let next = rows.iter().find(|row| row.empty > 0).map(|row| {
        (
            row.set_id.clone(),
            row.ident.clone(),
            row.name.clone(),
            first_empty(&row.slots),
        )
    });
    let open = move |set_id: String, ident: String, slot_index: u8| {
        let mut picker = picker;
        picker.set(Some(PickerTarget {
            powerset_id: set_id,
            power_internal_name: ident,
            slot_index,
            destination: PickerDestination::Build,
        }));
    };

    rsx! {
        div { class: "phone-job",
            p { class: "phone-job__note",
                "Tap a power to fill its empty slots, or one slot to change what is in it."
            }
            if empty_build {
                p { class: "phone-job__note", "Choose some powers first." }
            }
            ol { class: "phone-picks",
                for row in rows {
                    li { key: "{row.key}",
                        div { class: "phone-pick phone-pick--enhance",
                            button {
                                class: "phone-pick__head",
                                r#type: "button",
                                onclick: {
                                    let (set_id, ident, at) = (row.set_id.clone(), row.ident.clone(), first_empty(&row.slots));
                                    move |_| open(set_id.clone(), ident.clone(), at)
                                },
                                span { class: "phone-pick__level mono",
                                    if let Some(level) = row.level { "{level}" } else { "·" }
                                }
                                span { class: "phone-pick__name", "{row.name}" }
                                span { class: if row.empty > 0 { "phone-pick__open is-wanting" } else { "phone-pick__open" },
                                    if row.empty > 0 { "{row.empty} empty ›" } else { "Full ›" }
                                }
                            }
                            span { class: "phone-pick__enh",
                                for (index, slot) in row.slots.iter().cloned().enumerate() {
                                    button {
                                        key: "{index}",
                                        class: if slot.is_some() { "phone-enh" } else { "phone-enh is-empty" },
                                        r#type: "button",
                                        "aria-label": if slot.is_some() { "Slot {index + 1} of {row.name}" } else { "Empty slot {index + 1} of {row.name}" },
                                        onclick: {
                                            let (set_id, ident) = (row.set_id.clone(), row.ident.clone());
                                            move |_| open(set_id.clone(), ident.clone(), index as u8)
                                        },
                                        if let Some(url) = slot.as_deref().filter(|url| !url.is_empty()) {
                                            img { class: "phone-enh__art", src: "{url}", alt: "" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some((set_id, ident, name, at)) = next {
                div { class: "phone-job__foot",
                    button {
                        class: "phone-job__cta",
                        r#type: "button",
                        onclick: move |_| open(set_id.clone(), ident.clone(), at),
                        "Fill the next empty slots: {name}"
                    }
                }
            }
        }
    }
}
