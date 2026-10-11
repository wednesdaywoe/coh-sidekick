//! Which VEAT branch the Available rail shows, and the one way to change it.
//!
//! A VEAT's branch is chosen the way Mids chooses it: from the primary column's own select,
//! which lists the base primary beside each branch's primary (Widow Training, Night Widow
//! Training, Fortunata Training). The chosen branch's pair then reads as the lower half of the
//! two columns it extends, under a divider, instead of as four more columns beside them.
//!
//! The build still carries no branch field. A branch is BOUGHT INTO (`powers::branch_offers`
//! says why), so the branch a build holds is whichever one its picks are out of, and that is
//! what the select shows. Before the first branch pick there is nothing to read it from, and
//! the select only chooses which branch to browse: [`BranchView`] is view state, held beside
//! the picker's open target rather than in the build, and it never reaches a save.
//!
//! Switching away from a held branch drops its picks, slots and all, so it asks first. The
//! select has no seat wide enough to state that in, which is the case [`crate::confirm`]
//! exists for, and the modal is hosted at the shell root for the containment reason
//! [`crate::modal`] gives.

use crate::build_session::BuildSession;
use crate::shell::Db;
use coh_data::archetypes::ArchetypeBranch;
use coh_data::CharacterState;
use dioxus::prelude::*;

/// The branch to browse while the build holds no branch pick. An id from the archetype's
/// `branches`; `None` browses the base pair alone. A stale id (left from another archetype)
/// matches no branch and reads as `None`, so nothing has to clear it.
#[derive(Clone, Copy)]
pub struct BranchView(pub Signal<Option<String>>);

/// A switch away from a held branch, waiting on the confirm. `to` is the branch to show
/// afterwards, `None` for the base pair alone.
#[derive(Clone, PartialEq)]
pub struct BranchSwitch {
    pub to: Option<String>,
}

/// The pending switch, and a count bumped whenever one is answered. The select's per-option
/// `selected` cannot pull a native `<select>` back once the user has moved it, so a cancelled
/// switch would leave it showing the branch that was refused; keying the select on this count
/// remounts it onto what the build actually holds.
#[derive(Clone, Copy)]
pub struct PendingBranchSwitch {
    pub switch: Signal<Option<BranchSwitch>>,
    pub answered: Signal<u32>,
}

/// The select value standing for a branch, kept apart from the set ids the same select offers.
pub const BRANCH_VALUE_PREFIX: &str = "branch:";

/// The archetype's branches, empty for the archetypes without any and for a catalog that will
/// not parse (the select's own catalog read surfaces that fault).
pub fn archetype_branches(database: &Db, state: &CharacterState) -> Vec<ArchetypeBranch> {
    let Some(archetype_id) = state.archetype.id.as_deref() else {
        return Vec::new();
    };
    database
        .archetypes()
        .ok()
        .and_then(|catalog| catalog.get(archetype_id).map(|at| at.branches.clone()))
        .unwrap_or_default()
}

fn is_branch_set(branch: &ArchetypeBranch, set_id: &str) -> bool {
    [&branch.primary_set, &branch.secondary_set]
        .into_iter()
        .flatten()
        .any(|id| id.eq_ignore_ascii_case(set_id))
}

/// How many picks the build holds out of this branch's two sets. Branch picks sit in the role
/// lists under their own set id, so that id is the whole test.
pub fn branch_pick_count(branch: &ArchetypeBranch, state: &CharacterState) -> usize {
    state
        .primary
        .powers
        .iter()
        .chain(state.secondary.powers.iter())
        .filter(|power| is_branch_set(branch, &power.powerset))
        .count()
}

/// The branch the build's picks are out of. The game lets a build hold only one (each branch
/// set's `SpecializeRequires` names the others), so the first match is the only one; a build
/// read from a file that broke that rule still shows the branch it bought into first.
pub fn held_branch<'a>(
    branches: &'a [ArchetypeBranch],
    state: &CharacterState,
) -> Option<&'a ArchetypeBranch> {
    branches
        .iter()
        .find(|branch| branch_pick_count(branch, state) > 0)
}

/// The branch the rail shows: the held one, else the one being browsed.
pub fn shown_branch<'a>(
    branches: &'a [ArchetypeBranch],
    state: &CharacterState,
    browsed: Option<&str>,
) -> Option<&'a ArchetypeBranch> {
    held_branch(branches, state)
        .or_else(|| browsed.and_then(|id| branches.iter().find(|branch| branch.id == id)))
}

/// Drop every pick out of this branch's sets, the slots and enhancements with them.
pub fn remove_branch_picks(state: &mut CharacterState, branch: &ArchetypeBranch) {
    for role in [&mut state.primary, &mut state.secondary] {
        role.powers
            .retain(|power| !is_branch_set(branch, &power.powerset));
    }
}

/// What a choice on the primary select does, given the branch the build holds.
#[derive(Debug, PartialEq, Eq)]
pub enum BranchChoice {
    /// Show this branch (`None` for the base pair alone). Nothing in the build changes.
    Browse(Option<String>),
    /// The build holds picks out of another branch, so the choice waits on the confirm.
    Confirm(Option<String>),
}

/// Decide a primary-select choice. `to` is the branch chosen, `None` for the base set.
pub fn choose_branch(held: Option<&ArchetypeBranch>, to: Option<String>) -> BranchChoice {
    match held {
        Some(branch) if to.as_deref() != Some(branch.id.as_str()) => BranchChoice::Confirm(to),
        _ => BranchChoice::Browse(to),
    }
}

/// The confirm for a pending switch. Mounted at the shell root, outside both layout roots.
#[component]
pub fn BranchSwitchHost(database: Db) -> Element {
    let session = use_context::<BuildSession>();
    let pending = use_context::<PendingBranchSwitch>();
    let mut view = use_context::<BranchView>().0;
    let mut switch = pending.switch;
    let mut answered = pending.answered;

    let Some(BranchSwitch { to }) = switch() else {
        return rsx! {};
    };
    let branches = archetype_branches(&database, &session.build.read());
    let Some(held) = held_branch(&branches, &session.build.read()).cloned() else {
        // The picks went some other way (an undo) while the confirm was up; nothing to drop.
        switch.set(None);
        return rsx! {};
    };
    let count = branch_pick_count(&held, &session.build.read());
    let to_name = to
        .as_deref()
        .and_then(|id| branches.iter().find(|branch| branch.id == id))
        .map(|branch| branch.name.clone());
    let powers = if count == 1 { "power" } else { "powers" };
    let message = match &to_name {
        Some(name) => format!(
            "Switching to {name} removes the {count} {} {powers} this build holds, with their slots and enhancements.",
            held.name
        ),
        None => format!(
            "Leaving the {} branch removes the {count} {powers} this build holds from it, with their slots and enhancements.",
            held.name
        ),
    };

    rsx! {
        crate::confirm::ConfirmModal {
            title: match &to_name {
                Some(name) => format!("Switch to {name}?"),
                None => format!("Leave {}?", held.name),
            },
            message,
            confirm_label: "Remove and switch".to_string(),
            on_confirm: move |_| {
                let held = held.clone();
                let database = database.clone();
                session.commit(move |state| {
                    remove_branch_picks(state, &held);
                    crate::granted_powers::sync(state, &database);
                });
                view.set(to.clone());
                switch.set(None);
                answered += 1;
            },
            on_cancel: move |_| {
                switch.set(None);
                answered += 1;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::SelectedPower;

    fn branch(id: &str, primary: &str, secondary: &str) -> ArchetypeBranch {
        ArchetypeBranch {
            id: id.to_string(),
            name: id.to_string(),
            primary_set: Some(primary.to_string()),
            secondary_set: Some(secondary.to_string()),
        }
    }

    fn widow() -> Vec<ArchetypeBranch> {
        vec![
            branch(
                "night-widow",
                "arachnos-widow/night-widow-training",
                "arachnos-widow/widow-teamwork",
            ),
            branch(
                "fortunata",
                "arachnos-widow/fortunata-training",
                "arachnos-widow/fortunata-teamwork",
            ),
        ]
    }

    fn build(picks: &[(&str, &str)]) -> CharacterState {
        let mut state = CharacterState::empty(coh_data::DatasetId::Homecoming);
        state.primary.id = Some("arachnos-widow/widow-training".to_string());
        state.secondary.id = Some("arachnos-widow/teamwork".to_string());
        for (set, power) in picks {
            let pick = SelectedPower::picked(*power, *set, 1);
            if set.contains("training") {
                state.primary.powers.push(pick);
            } else {
                state.secondary.powers.push(pick);
            }
        }
        state
    }

    #[test]
    fn the_held_branch_outranks_the_browsed_one() {
        let branches = widow();
        let state = build(&[("arachnos-widow/widow-teamwork", "NW_Mind_Link")]);
        let shown = shown_branch(&branches, &state, Some("fortunata"));
        assert_eq!(shown.map(|b| b.id.as_str()), Some("night-widow"));
    }

    #[test]
    fn with_no_branch_pick_the_browsed_branch_shows() {
        let branches = widow();
        let state = build(&[("arachnos-widow/widow-training", "Swipe")]);
        assert_eq!(
            shown_branch(&branches, &state, Some("fortunata")).map(|b| b.id.as_str()),
            Some("fortunata")
        );
        assert!(shown_branch(&branches, &state, None).is_none());
        // A stale id from another archetype browses nothing rather than failing.
        assert!(shown_branch(&branches, &state, Some("bane-spider")).is_none());
    }

    #[test]
    fn switching_away_from_a_held_branch_asks_and_staying_does_not() {
        let branches = widow();
        let held = Some(&branches[0]);
        assert_eq!(
            choose_branch(held, Some("fortunata".into())),
            BranchChoice::Confirm(Some("fortunata".into()))
        );
        assert_eq!(choose_branch(held, None), BranchChoice::Confirm(None));
        assert_eq!(
            choose_branch(held, Some("night-widow".into())),
            BranchChoice::Browse(Some("night-widow".into()))
        );
        assert_eq!(
            choose_branch(None, Some("fortunata".into())),
            BranchChoice::Browse(Some("fortunata".into()))
        );
    }

    #[test]
    fn removing_a_branch_keeps_the_base_picks() {
        let branches = widow();
        let mut state = build(&[
            ("arachnos-widow/widow-training", "Swipe"),
            ("arachnos-widow/teamwork", "Foresight"),
            ("arachnos-widow/night-widow-training", "NW_Eviscerate"),
            ("arachnos-widow/widow-teamwork", "NW_Mind_Link"),
        ]);
        assert_eq!(branch_pick_count(&branches[0], &state), 2);
        remove_branch_picks(&mut state, &branches[0]);
        let left: Vec<&str> = state
            .all_selected()
            .map(|p| p.internal_name.as_str())
            .collect();
        assert_eq!(left, ["Swipe", "Foresight"]);
        assert!(held_branch(&branches, &state).is_none());
    }
}
