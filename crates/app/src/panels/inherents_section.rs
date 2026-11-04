//! One granted-inherent group as a panel of its own — the powers the game hands the
//! character, which spend no power pick.
//!
//! **Why a panel each.** These used to be one section inside the Powers panel, drawn as five
//! fixed columns in an order nothing could change, with one fold for the whole section. The
//! order of the groups is a real preference (a build that lives on Health and Stamina wants
//! Fitness above the archetype's own inherent; a build that never touches Brawl wants Basic
//! out of the way), and the Powers panel had no way to express either. As panels they are
//! placed, folded, hidden and popped out by the same grid every other surface uses, and the
//! Powers panel stops drawing them at all — one place to see a card, not two.
//!
//! This is the beta's arrangement: `inherent-fitness`, `inherent-basic`, `inherent-prestige`
//! and `inherent-archetype` were planner columns there, each placeable and hideable on its
//! own.
//!
//! **What a group holds.** The build's inherents whose [`InherentCategory`] is this group's.
//! The category is stamped by the reconcile that materializes them
//! ([`crate::inherents::sync`] and [`crate::granted_powers::sync`]), so this module reads it
//! and never guesses: a power with no category belongs to no group and is drawn nowhere,
//! which is the honest answer for a build mid-load.
//!
//! **The archetype's own inherent** is the one group whose emptiness is worth a sentence. The
//! dataset may declare an inherent it ships no power for — Thunderspy's Primalist is the
//! corpus's one case, and it is the export's answer rather than a defect (INHERENT-10) — so
//! the panel says so instead of drawing an empty box.

use crate::build_session::BuildSession;
use crate::panels::power_group::PowerGroup;
use crate::panels::powers::PickedPowerCard;
use crate::shell::Db;
use coh_data::InherentCategory;
use dioxus::prelude::*;

/// Whether a group's cards start folded.
///
/// Basic and Prestige hold the powers every character is handed and almost no build plans
/// around — Brawl, Rest, the Sprints — so they open folded and give the room to the groups a
/// build does work with. The archetype's own inherent and the fitness powers stay open: both
/// carry powers a build plans around (fitness takes slots, the archetype inherent drives the
/// AT's whole mechanic), and the beta's own bug report for folding those away was players
/// concluding the power was missing entirely.
///
/// Exhaustive with no `_` arm, so a new [`InherentCategory`] has to state its own answer.
fn group_starts_collapsed(group: InherentCategory) -> bool {
    match group {
        // Granted holds the pick-gated grants — the Kheldian form attacks a build slots and
        // plans around, so it opens like the other two plannable groups.
        InherentCategory::Archetype | InherentCategory::Granted | InherentCategory::Fitness => {
            false
        }
        InherentCategory::Basic | InherentCategory::Prestige => true,
    }
}

/// One group's panel body: its cards, and the sentence a group with nothing to draw owes the
/// reader.
#[component]
pub fn InherentGroupPanel(database: Db, group: InherentCategory) -> Element {
    let session = use_context::<BuildSession>();
    let build = session.build;
    let powers: Vec<coh_data::SelectedPower> = build
        .read()
        .inherents
        .iter()
        .filter(|power| power.inherent_category == Some(group))
        .cloned()
        .collect();

    // The archetype names an inherent; the dataset may or may not ship a power for it. The
    // corpus has one absence and it is Thunderspy's Primalist, which is the export's answer
    // rather than a defect: that fork grants and spends Primal Energy from the archetype's own
    // powersets, and files only a meter and a dampen under the name (INHERENT-10).
    //
    // Shown as a marked row either way, because from here the two are the same lookup coming
    // back empty — the verdict lives on the TS side, which the contract does not carry. So the
    // row states the absence and stops there. It used to add "so it contributes nothing to the
    // build", which is exactly the part this lookup cannot know and is false for the one case
    // in the corpus.
    let missing_archetype_inherent = (group == InherentCategory::Archetype)
        .then(|| crate::inherents::declared_inherent_name(&build.read(), &database))
        .flatten()
        .filter(|_| crate::inherents::archetype_inherent(&build.read(), &database).is_none());

    // What the user has hidden, and what this group still draws. The hidden ones are named in
    // the footer below rather than dropped: a hidden power is still in the build and still
    // contributing, so a panel that simply did not mention it would be the one place the user
    // could not find out that it was there.
    let hidden = use_context::<crate::hidden_inherents_store::HiddenInherents>().0;
    let hidden_here: Vec<coh_data::SelectedPower> = powers
        .iter()
        .filter(|power| {
            crate::hidden_inherents_store::is_hidden(&hidden.read(), &power.internal_name)
        })
        .cloned()
        .collect();
    let shown: Vec<coh_data::SelectedPower> = powers
        .iter()
        .filter(|power| {
            !crate::hidden_inherents_store::is_hidden(&hidden.read(), &power.internal_name)
        })
        .cloned()
        .collect();

    if powers.is_empty() && missing_archetype_inherent.is_none() {
        return rsx! {
            p { class: "inherents__empty", "No {crate::grid::model::inherent_group_title(group).to_lowercase()} in this build." }
        };
    }

    rsx! {
        div { class: "inherent-group",
            if let Some(name) = missing_archetype_inherent {
                div { class: "load-state error",
                    "This dataset ships no inherent power named {name}, so this row carries "
                    "no numbers. Whatever the mechanic does comes from elsewhere in the build."
                }
            }
            PowerGroup {
                title: crate::grid::model::inherent_group_title(group).to_string(),
                count: shown.len(),
                starts_collapsed: group_starts_collapsed(group),
                for (index, power) in shown.iter().enumerate() {
                    PickedPowerCard {
                        key: "inherent-{index}",
                        database: database.clone(),
                        power: power.clone(),
                        powerset_id: coh_data::INHERENT_SET.to_string(),
                    }
                }
            }
            if !hidden_here.is_empty() {
                HiddenInherentsFooter { database: database.clone(), powers: hidden_here }
            }
        }
    }
}

/// The line under a group naming the powers it is hiding, each with the button that brings it
/// back.
///
/// It exists because hiding is display-only: the power keeps its slotting and keeps feeding the
/// totals, and a hidden power that is switched ON has no card left to switch it off. A footer
/// that named nothing would be chrome; one that names what is gone is the way back, in the same
/// place the user last saw the card.
#[component]
fn HiddenInherentsFooter(database: Db, powers: Vec<coh_data::SelectedPower>) -> Element {
    let mut hidden = use_context::<crate::hidden_inherents_store::HiddenInherents>().0;
    rsx! {
        div { class: "inherents__hidden",
            span { class: "inherents__hidden-label",
                if powers.len() == 1 { "1 hidden:" } else { "{powers.len()} hidden:" }
            }
            for power in powers.iter() {
                {
                    let name = power.internal_name.clone();
                    // The name the card wore, not the internal one: `Prestige_Ninja_Run` is
                    // what the build stores and "Ninja Run" is what the user hid.
                    let label = crate::view::power_view::resolve_power_def(
                        &database,
                        &power.powerset,
                        &power.internal_name,
                    )
                    .map(|def| def.name.clone())
                    .unwrap_or_else(|| power.internal_name.replace('_', " "));
                    rsx! {
                        button {
                            key: "show-{name}",
                            class: "inherents__show",
                            r#type: "button",
                            title: "Show {label} again",
                            onclick: move |_| {
                                hidden.write().remove(&name);
                            },
                            "{label}"
                            span { class: "inherents__show-mark", "aria-hidden": "true", "↺" }
                        }
                    }
                }
            }
        }
    }
}
