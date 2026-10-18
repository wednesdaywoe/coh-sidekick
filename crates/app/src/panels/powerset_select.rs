//! The primary/secondary powerset choice — the control, its roster, and the one write path
//! both surfaces that offer it go through.
//!
//! Two surfaces offer the choice: the header's Build Identity popover, where it sits with the
//! archetype that gates it, and the Available rail's column heads, where the set being chosen
//! is the one being read (LAY4). They are two frames around one control, never two controls:
//! the roster, the set-level gate, the name resolution and the commit all live here, and a
//! caller supplies only which role it is asking for and what class the `<select>` wears.
//!
//! That is not tidiness. `identity::set_archetype` clears `primary` and `secondary` together
//! because a different archetype offers different sets, and a second control writing those
//! fields on its own path would keep offering the old archetype's roster after the switch —
//! the desync this module exists to make unrepresentable.

use crate::build_session::BuildSession;
use crate::icon_select::{IconOption, IconSelect};
use crate::panels::branch_view;
use crate::panels::powers::BuildRole;
use crate::shell::Db;
use crate::view::icons;
use coh_data::PowersetSelection;
use dioxus::prelude::*;

/// One `<option>`-worth of powerset choice: its id (the `<select>` value), display name, and
/// the set-level refusal that makes it unchoosable, if there is one.
#[derive(Clone, PartialEq)]
pub struct SetChoice {
    pub id: String,
    pub name: String,
    /// The game's own `SetBuyRequiresFailedText`, or the fault text of a gate that would not
    /// read. `None` where the set is choosable.
    pub blocked: Option<String>,
}

/// The choosable sets from one of an archetype's rosters, with each one's set-level verdict.
///
/// No primary or secondary set on any of the three forks carries a set-level gate today, so
/// this changes nothing anyone can see — it is here because a set the game refuses must not be
/// silently offered (Rule 1). Graded by a synthetic gate, since the corpus holds no violating
/// case — see the tests below.
///
/// The sets that DO carry the gate are the VEAT branch pairs, and they are not rostered here:
/// nothing declares a branch, the game confers the set when a power is bought out of it. The
/// rail's primary select still lists them, as a choice of which branch to SHOW
/// ([`crate::panels::branch_view`]), but that choice writes no set into the build.
///
/// The listed-vs-disabled split is the game's (`Game/src/UI/uiPowers.c:395`): a refused
/// specialization set is dropped from the list, and any other refusal is drawn disabled with
/// its message.
pub fn set_choices(
    database: &Db,
    set_ids: &[String],
    state: &coh_data::CharacterState,
) -> Vec<SetChoice> {
    let mut choices: Vec<SetChoice> = set_ids
        .iter()
        .filter_map(|set_id| {
            let Some(powerset) = database.find_powerset(set_id) else {
                // A roster naming a set the dataset does not ship is a fault worth seeing, not
                // a row to drop — the option renders disabled saying so.
                return Some(SetChoice {
                    id: set_id.clone(),
                    name: set_id.clone(),
                    blocked: Some("not in this dataset".to_string()),
                });
            };
            let verdict = coh_data::set_gate(
                &powerset.buy_requires,
                &powerset.buy_requires_failed,
                powerset.specialize_at,
                &powerset.specialize_requires,
                state,
                state.archetype.id.as_deref(),
                &database.set_paths,
            );
            let blocked = match verdict {
                Ok(coh_data::SetGate::Open) => None,
                Ok(coh_data::SetGate::Closed { reason }) => Some(reason),
                // Specialization sets: refused means not listed at all.
                Ok(coh_data::SetGate::BranchClosed | coh_data::SetGate::NotYet { .. }) => {
                    return None
                }
                Err(error) => Some(error.to_string()),
            };
            Some(SetChoice {
                id: set_id.clone(),
                name: powerset.name.clone(),
                blocked,
            })
        })
        .collect();
    // The roster comes in the data's order: internal names ("Devices" is Gadgets), with
    // later additions like Atomic Manipulation appended. The menu reads by display name.
    choices.sort_by(|left, right| left.name.cmp(&right.name));
    choices
}

/// Build a [`PowersetSelection`] for the chosen set id (empty id ⇒ cleared selection),
/// resolving its display name from the choices list or the dataset.
pub fn resolve_set(choices: &[SetChoice], database: &Db, set_id: &str) -> PowersetSelection {
    if set_id.is_empty() {
        return PowersetSelection::default();
    }
    let name = choices
        .iter()
        .find(|choice| choice.id == set_id)
        .map(|choice| choice.name.clone())
        .or_else(|| database.find_powerset(set_id).map(|ps| ps.name.clone()))
        .unwrap_or_else(|| set_id.to_string());
    PowersetSelection {
        id: Some(set_id.to_string()),
        name,
        powers: Vec::new(),
    }
}

/// The control itself — the whole of the powerset choice, wrapped by whatever frame the
/// calling surface wants around it.
///
/// It reads its own roster rather than taking one, which is what makes it the single write
/// path rather than a shared widget two callers feed: a caller that assembled the roster could
/// assemble a stale one, and the desync in this module's header is exactly that bug.
///
/// `class` is the frame's, because the two frames are genuinely different objects — a labelled
/// field in a form, and the head of a column in the rail — and nothing but the styling differs.
#[component]
pub fn PowersetSelect(
    database: Db,
    role: BuildRole,
    /// The button's class, so a caller can frame the control without owning it.
    class: String,
    /// What an empty selection reads as. The rail's heads say it in fewer words than the
    /// popover's fields, because a column head is not a form label.
    placeholder: String,
    /// List the archetype's VEAT branches after the primary sets, Mids-style, so the column
    /// that shows a branch is the one that chooses it. The rail's primary head only: the
    /// branch it chooses is the rail's view ([`crate::panels::branch_view`]), and the
    /// popover has no branch half to show.
    #[props(default)]
    offer_branches: bool,
) -> Element {
    let session = use_context::<BuildSession>();
    let build = session.build;

    // The archetype catalog is UI-only and parsed on demand, so memoize the one parse per
    // dataset. An `Err` surfaces as a visible node rather than a panic (Rule 1's UI edge).
    let catalog = use_memo(use_reactive!(|database| database.archetypes()));

    // The roster, with each set's set-level verdict. Recomputed when the build or the dataset
    // changes — the verdict reads what the build holds, so it cannot be memoized on the
    // archetype alone.
    let roster_db = database.clone();
    let choices = use_memo(use_reactive!(|(roster_db, role)| {
        let state = build.read();
        let (Some(id), Ok(catalog)) = (state.archetype.id.clone(), &*catalog.read()) else {
            return Vec::<SetChoice>::new();
        };
        let Some(at) = catalog.get(&id) else {
            return Vec::new();
        };
        let roster = match role {
            BuildRole::Primary => &at.primary_sets,
            BuildRole::Secondary => &at.secondary_sets,
        };
        set_choices(&roster_db, roster, &state)
    }));

    if let Err(error) = &*catalog.read() {
        return rsx! {
            div { class: "load-state error", "Archetype data failed to load: {error}" }
        };
    }

    let current = match role {
        BuildRole::Primary => build.read().primary.id.clone(),
        BuildRole::Secondary => build.read().secondary.id.clone(),
    }
    .unwrap_or_default();
    let has_archetype = build.read().archetype.id.is_some();
    let choices = choices();

    // The branch options, and which one the select stands on.
    // Read unconditionally (hooks keep their order), then kept only where the prop asks.
    let view_ctx = try_use_context::<branch_view::BranchView>();
    let pending_ctx = try_use_context::<branch_view::PendingBranchSwitch>();
    let branch_ctx = view_ctx
        .zip(pending_ctx)
        .filter(|_| offer_branches && role == BuildRole::Primary)
        .map(|(view, pending)| (view.0, pending));
    let branches = if branch_ctx.is_some() {
        branch_view::archetype_branches(&database, &build.read())
    } else {
        Vec::new()
    };
    let shown_id = branch_ctx.and_then(|(browsed, _)| {
        branch_view::shown_branch(&branches, &build.read(), browsed.read().as_deref())
            .map(|branch| branch.id.clone())
    });
    let held_id = branch_view::held_branch(&branches, &build.read()).map(|b| b.id.clone());
    let branch_options: Vec<(String, String)> = branches
        .iter()
        .map(|branch| {
            let name = branch
                .primary_set
                .as_deref()
                .and_then(|set| database.find_powerset(set))
                .map_or_else(|| branch.name.clone(), |set| set.name.clone());
            (
                format!("{}{}", branch_view::BRANCH_VALUE_PREFIX, branch.id),
                name,
            )
        })
        .collect();
    // What the select stands on: the shown branch when there is one, else the set itself.
    let current_value = match &shown_id {
        Some(id) => format!("{}{id}", branch_view::BRANCH_VALUE_PREFIX),
        None => current.clone(),
    };
    // Bumped when a switch is answered, so a refused switch remounts the select onto the
    // branch the build still holds (see `PendingBranchSwitch`).
    let answered = branch_ctx.map_or(0, |(_, pending)| (pending.answered)());
    let options: Vec<IconOption> = choices
        .iter()
        .map(|choice| IconOption {
            value: choice.id.clone(),
            label: choice.name.clone(),
            icon: database
                .find_powerset(&choice.id)
                .and_then(icons::powerset_icon_url),
            // Still listed when it is what the build holds: a set can be refused AFTER it was
            // chosen, and dropping it would silently rewrite the build to explain a rule.
            blocked: choice.blocked.clone(),
        })
        .chain(
            branches
                .iter()
                .zip(&branch_options)
                .map(|(branch, (value, name))| {
                    IconOption::new(
                        value.clone(),
                        name.clone(),
                        branch
                            .primary_set
                            .as_deref()
                            .and_then(|set| database.find_powerset(set))
                            .and_then(icons::powerset_icon_url),
                    )
                }),
        )
        .collect();

    rsx! {
        for answered in std::iter::once(answered) {
        IconSelect {
            key: "{answered}",
            class: class.clone(),
            disabled: !has_archetype,
            aria_label: match role {
                BuildRole::Primary => "Primary powerset".to_string(),
                BuildRole::Secondary => "Secondary powerset".to_string(),
            },
            placeholder: placeholder.clone(),
            value: current_value.clone(),
            options: options.clone(),
            onchange: {
                let choices = choices.clone();
                let database = database.clone();
                let current = current.clone();
                let held_id = held_id.clone();
                let branches = branches.clone();
                move |value: String| {
                    // A branch option, or the set itself while a branch is shown: a choice of
                    // which branch to show, which only reaches the build through the confirm.
                    if let Some((mut browsed, pending)) = branch_ctx {
                        let to = value
                            .strip_prefix(branch_view::BRANCH_VALUE_PREFIX)
                            .map(str::to_string);
                        if to.is_some() || value == current {
                            let held = held_id
                                .as_deref()
                                .and_then(|id| branches.iter().find(|b| b.id == id));
                            let mut switch = pending.switch;
                            match branch_view::choose_branch(held, to) {
                                branch_view::BranchChoice::Browse(to) => browsed.set(to),
                                branch_view::BranchChoice::Confirm(to) => {
                                    switch.set(Some(branch_view::BranchSwitch { to }))
                                }
                            }
                            return;
                        }
                    }
                    let selection = resolve_set(&choices, &database, &value);
                    let database = database.clone();
                    session.commit(move |state| {
                        match role {
                            BuildRole::Primary => state.primary = selection,
                            BuildRole::Secondary => state.secondary = selection,
                        }
                        crate::granted_powers::sync(state, &database);
                    });
                }
            },
        }
        }
    }
}
