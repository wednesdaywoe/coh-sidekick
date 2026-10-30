//! The Convert to another archetype dialog: pick an archetype that shares a power set with this
//! build, read what the copy keeps, moves and leaves behind, then confirm.
//!
//! Hosted at the shell root like the pool picker, so its backdrop is not clipped by the identity
//! popover that opens it. The copy replaces the build on screen through
//! [`BuildSession::commit`], as New build does, so the original is one Undo away; the report says
//! so, and says to save first to keep both.

use crate::archetype_convert::{self, Conversion, DropWhy};
use crate::build_session::BuildSession;
use crate::icon_select::{IconOption, IconSelect};
use crate::modal::{Modal, ModalSize};
use crate::shell::Db;
use crate::view::icons;
use dioxus::prelude::*;

/// The dialog's open flag, provided at the shell root.
#[derive(Clone, Copy)]
pub struct ConvertOpen(pub Signal<bool>);

#[component]
pub fn ConvertHost(database: Db) -> Element {
    let mut open = use_context::<ConvertOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Convert to another archetype".to_string(),
            size: ModalSize::Lg,
            on_close: move |_| open.set(false),
            ConvertBody { database }
        }
    }
}

#[component]
fn ConvertBody(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let mut open = use_context::<ConvertOpen>().0;
    let mut target = use_signal(String::new);

    let Ok(catalog) = database.archetypes() else {
        return rsx! { p { class: "convert__note", "Archetype data failed to load." } };
    };
    let build = session.build.read().clone();
    // An imported build can carry the archetype id without its name; the catalog has both.
    let from = build
        .archetype
        .id
        .as_deref()
        .and_then(|id| catalog.get(id))
        .map_or_else(|| build.archetype.name.clone(), |at| at.name.clone());
    // Only archetypes sharing a set with this build: converting to any other keeps nothing.
    let offered: Vec<(&coh_data::Archetype, Vec<String>)> = catalog
        .all()
        .iter()
        .filter(|at| build.archetype.id.as_deref() != Some(at.id.as_str()))
        .map(|at| (at, archetype_convert::shared_sets(&build, &database, at)))
        .filter(|(_, shared)| !shared.is_empty())
        .collect();
    let options: Vec<IconOption> = offered
        .iter()
        .map(|(at, shared)| {
            IconOption::new(
                &at.id,
                format!("{} · keeps {}", at.name, shared.join(", ")),
                icons::archetype_icon_url(at),
            )
        })
        .collect();
    let conversion = (!target().is_empty())
        .then(|| archetype_convert::convert_build(&build, &database, &target()))
        .flatten();
    let to = catalog
        .get(&target())
        .map(|at| at.name.clone())
        .unwrap_or_default();
    let new_inherent = catalog
        .get(&target())
        .map(|at| at.inherent.name.clone())
        .unwrap_or_default();

    rsx! {
        div { class: "convert",
            p { class: "convert__note",
                "Copies this {from} build to another archetype. Power sets both archetypes have come across with their picks and slotting; the rest is listed below before anything changes."
            }
            if offered.is_empty() {
                p { class: "convert__note",
                    "No other archetype has this build's power sets."
                }
            } else {
                div { class: "build-field",
                    span { class: "field-label", "Convert to" }
                    IconSelect {
                        class: "build-select".to_string(),
                        aria_label: "Convert to".to_string(),
                        placeholder: "— choose archetype —".to_string(),
                        value: target(),
                        options,
                        onchange: move |id: String| target.set(id),
                    }
                }
            }
            if let Some(conversion) = conversion.as_ref() {
                Report { conversion: conversion.clone(), to: to.clone(), new_inherent }
                p { class: "convert__note",
                    "The copy replaces the build on screen. Undo brings the {from} build back; to keep both, save it first (File → Save file)."
                }
            }
            div { class: "confirm__actions",
                button {
                    class: "seg",
                    r#type: "button",
                    onclick: move |_| open.set(false),
                    "Cancel"
                }
                button {
                    class: "seg is-primary",
                    r#type: "button",
                    disabled: conversion.is_none(),
                    onclick: move |_| {
                        if let Some(conversion) = conversion.clone() {
                            session.commit(move |working| *working = conversion.build);
                            // A different build now: its short link must not update the original's row.
                            crate::cloud::owner_store::clear_quick_share();
                            open.set(false);
                        }
                    },
                    if to.is_empty() {
                        "Convert"
                    } else {
                        "Convert to {to}"
                    }
                }
            }
        }
    }
}

#[component]
fn Report(conversion: Conversion, to: String, new_inherent: String) -> Element {
    let kept: Vec<String> = conversion
        .sets
        .iter()
        .filter_map(|set| {
            let role = set.to?;
            Some(if role == set.from {
                format!("{} (stays {})", set.name, role.label())
            } else {
                format!(
                    "{} ({} becomes {})",
                    set.name,
                    set.from.label(),
                    role.label()
                )
            })
        })
        .collect();
    let empty: Vec<&str> = conversion.empty.iter().map(|role| role.label()).collect();
    let groups = left_behind(&conversion, &to);
    let pieces = pieces_by_power(&conversion);

    rsx! {
        div { class: "convert__report",
            Section { title: "Comes across".to_string(),
                li { "{kept.join(\" · \")}" }
                li { "Power pools, accolades and Incarnate powers" }
            }
            if !empty.is_empty() {
                Section { title: "Left for you to pick".to_string(),
                    for role in empty {
                        li { class: "convert__empty", "The {to} {role} set" }
                    }
                }
            }
            if !conversion.moved.is_empty() {
                Section { title: format!("Moved to a level {to} allows ({})", conversion.moved.len()),
                    for moved in conversion.moved.iter() {
                        li { "{moved.power}: level {moved.from} → {moved.to}" }
                    }
                }
            }
            if !groups.is_empty() || conversion.inherent.is_some() {
                Section {
                    title: format!("Left behind ({})", conversion.dropped.len()),
                    for (reason, powers) in groups {
                        li {
                            span { class: "convert__reason", "{reason}: " }
                            "{powers.join(\", \")}"
                        }
                    }
                    if let Some(inherent) = conversion.inherent.as_ref() {
                        li {
                            span { class: "convert__reason", "Inherent: " }
                            "{inherent} becomes {new_inherent}"
                        }
                    }
                }
            }
            if !pieces.is_empty() {
                Section {
                    title: format!(
                        "Enhancements taken out ({}); their slots stay, empty",
                        conversion.pieces.len(),
                    ),
                    for (power, names) in pieces {
                        li {
                            span { class: "convert__reason", "{power}: " }
                            "{names.join(\", \")}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Section(title: String, children: Element) -> Element {
    rsx! {
        section { class: "convert__section",
            h4 { class: "convert__title", "{title}" }
            ul { class: "convert__list", {children} }
        }
    }
}

/// Dropped powers grouped under one line per set and reason, in report order.
fn left_behind(conversion: &Conversion, to: &str) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for drop in &conversion.dropped {
        let reason = match drop.why {
            DropWhy::SetNotShared => format!("{} (no {to} version of the set)", drop.set),
            DropWhy::NoMatch => format!("{} (not in the {to} version)", drop.set),
            DropWhy::Refused => format!("{} ({to} can't take it)", drop.set),
            DropWhy::NoPickLeft => format!("{} (no power pick left for it)", drop.set),
        };
        match groups.iter_mut().find(|(seen, _)| *seen == reason) {
            Some((_, powers)) => powers.push(drop.power.clone()),
            None => groups.push((reason, vec![drop.power.clone()])),
        }
    }
    groups
}

/// Removed pieces grouped by power, with a set's pieces counted rather than each named.
fn pieces_by_power(conversion: &Conversion) -> Vec<(String, Vec<String>)> {
    let mut powers: Vec<(String, Vec<(String, usize)>)> = Vec::new();
    for piece in &conversion.pieces {
        let name = piece.set.clone().unwrap_or_else(|| piece.piece.clone());
        let at = match powers.iter().position(|(power, _)| *power == piece.power) {
            Some(at) => at,
            None => {
                powers.push((piece.power.clone(), Vec::new()));
                powers.len() - 1
            }
        };
        let names = &mut powers[at].1;
        match names.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => names.push((name, 1)),
        }
    }
    powers
        .into_iter()
        .map(|(power, names)| {
            let names = names
                .into_iter()
                .map(|(name, count)| match count {
                    1 => name,
                    _ => format!("{name} ×{count}"),
                })
                .collect();
            (power, names)
        })
        .collect()
}
