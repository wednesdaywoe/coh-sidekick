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
use crate::panels::powers::{BuildBudget, PickSheetLevel};
use crate::panels::powers_by_level::{deal_picks, PickCell};
use crate::shell::Db;
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

pub fn use_phone_job_provider() {
    let job = use_signal(|| Job::Powers);
    use_context_provider(|| PhoneJob(job));
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

/// The switch: three buttons, each with how far that pass has got.
#[component]
pub fn JobSwitch(database: Option<Db>) -> Element {
    let mut job = use_context::<PhoneJob>().0;
    let session = use_context::<BuildSession>();
    let slot_budget = use_context::<BuildBudget>().0;
    let (powers, enhance) = {
        let build = session.build.read();
        let pick_budget = database
            .as_ref()
            .and_then(|db| db.leveling_schedule.as_ref())
            .map(|schedule| schedule.total_power_picks_at_level(build.level));
        let picks = build.picked_powers().count();
        let (filled, total) = build
            .picked_powers()
            .fold((0, 0), |(filled, total), power| {
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
