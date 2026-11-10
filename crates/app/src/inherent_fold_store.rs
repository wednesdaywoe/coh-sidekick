//! Whether each inherent panel's cards are open or folded, kept across reloads on the same
//! `localStorage` channel as [`crate::hidden_inherents_store`].
//!
//! **A preference, not build state** — the same argument [`crate::hidden_inherents_store`]
//! makes for hiding: folding a panel is a statement about the window ("I never look at Brawl"),
//! it applies to every build the user opens, and putting it on [`coh_data::CharacterState`]
//! would make one reader's folded panels travel to every other reader of a shared build.
//!
//! **Why this store exists when [`crate::panels::power_group::PowerGroup`] calls its fold
//! deliberately unpersisted.** That note was written when the groups were a section of the
//! Powers panel, where the fold is the panel's own arrangement and a "not right now" is the
//! honest reading. Since the inherent groups became panels of their own (merged 2026-11-05)
//! every other property of the placement persists — where the panel sits, which powers it
//! hides, whether it is pinned — and the fold was the one that reset, which read as the app
//! forgetting (owner request 2026-11-10: "I sometimes slot inherents, but find that I have to
//! open them every time I open up Sidekick"). So the five inherent panels pass a caller-owned
//! fold and this store keeps it, while the groups inside the Powers panel and the by-level
//! columns keep the session-local fold.
//!
//! **Seeded with the authored defaults.** A group the store says nothing about keeps the
//! answer [`default_folded`] gives — Basic and Prestige folded, the rest open — so a fresh
//! profile opens exactly as it did before this store existed, and the first fold or expand
//! writes the memory.

use coh_data::InherentCategory;
use dioxus::prelude::*;

const KEY_INHERENT_FOLDS: &str = "sk-inherent-folds";

/// The fold state, provided once at the shell root. `true` = folded. A group missing from the
/// map is on its authored default, which is the state a fresh profile is in.
#[derive(Clone, Copy)]
pub struct InherentFolds(pub Signal<std::collections::BTreeMap<InherentCategory, bool>>);

/// Whether a group's cards are folded when nothing is stored: the authored defaults.
///
/// Basic and Prestige hold the powers every character is handed and almost no build plans
/// around — Brawl, Rest, the Sprints — so they open folded and give the room to the groups a
/// build does work with. The archetype's own inherent and the fitness powers stay open: both
/// carry powers a build plans around (fitness takes slots, the archetype inherent drives the
/// AT's whole mechanic), and the beta's own bug report for folding those away was players
/// concluding the power was missing entirely.
///
/// Exhaustive with no `_` arm, so a new [`InherentCategory`] has to state its own answer.
pub fn default_folded(group: InherentCategory) -> bool {
    match group {
        // Granted holds the pick-gated grants — the Kheldian form attacks a build slots and
        // plans around, so it opens like the other two plannable groups.
        InherentCategory::Archetype | InherentCategory::Granted | InherentCategory::Fitness => {
            false
        }
        InherentCategory::Basic | InherentCategory::Prestige => true,
    }
}

/// The fold for one group: the stored answer when there is one, the authored default when not.
pub fn folded(
    folds: &std::collections::BTreeMap<InherentCategory, bool>,
    group: InherentCategory,
) -> bool {
    folds
        .get(&group)
        .copied()
        .unwrap_or_else(|| default_folded(group))
}

/// One inherent group's fold, read and written through the shared persisted map.
///
/// Handed to [`crate::panels::power_group::PowerGroup`] as its `fold` prop: the group reads
/// [`Self::folded`] every render and [`Self::toggle`] writes the map, which the shell's effect
/// persists — the same division every other preference store uses.
///
/// `PartialEq` because the component macro memoizes props with it; two handles over the same
/// signal and group are the same prop.
#[derive(Clone, Copy, PartialEq)]
pub struct InherentFold {
    folds: Signal<std::collections::BTreeMap<InherentCategory, bool>>,
    group: InherentCategory,
}

impl InherentFold {
    pub fn new(
        folds: Signal<std::collections::BTreeMap<InherentCategory, bool>>,
        group: InherentCategory,
    ) -> Self {
        Self { folds, group }
    }

    /// Whether the group's cards are folded right now.
    pub fn folded(&self) -> bool {
        folded(&self.folds.read(), self.group)
    }

    /// Fold the group's cards, or open them, whichever it is not. The write lands in the map;
    /// persisting it is the shell's effect, like every other preference.
    ///
    /// Takes `self` by value — the handle is `Copy`, and [`dioxus::prelude::Writable::write`]
    /// wants an owned signal, which `&self` could not hand it.
    pub fn toggle(mut self) {
        let next = !self.folded();
        self.folds.write().insert(self.group, next);
    }
}

/// Persist the map (fire-and-forget, like [`crate::hidden_inherents_store::persist`]).
///
/// Written as one JSON object keyed by the category's lowercase names. Anything unreadable on
/// the way back in is treated as nothing stored — the defaults are the honest answer, and a
/// corrupt key must not fold panels the user left open.
pub fn persist(folds: &std::collections::BTreeMap<InherentCategory, bool>) {
    let Ok(json) = serde_json::to_string(folds) else {
        return;
    };
    let js =
        format!("try {{ localStorage.setItem({KEY_INHERENT_FOLDS:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

/// The persisted map; `None` (missing, or anything we didn't write) keeps the defaults.
pub async fn load() -> Option<std::collections::BTreeMap<InherentCategory, bool>> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_INHERENT_FOLDS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    let text = value.as_str()?;
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The authored defaults: the groups a build plans around open, the ones almost no build
    /// touches folded. If this drifts, a fresh profile changes shape with no store behind it.
    #[test]
    fn defaults_fold_basic_and_prestige_only() {
        let folded: Vec<_> = [
            InherentCategory::Fitness,
            InherentCategory::Basic,
            InherentCategory::Prestige,
            InherentCategory::Archetype,
            InherentCategory::Granted,
        ]
        .into_iter()
        .filter(|group| default_folded(*group))
        .collect();
        assert_eq!(
            folded,
            vec![InherentCategory::Basic, InherentCategory::Prestige]
        );
    }

    /// A stored answer wins over the default, and a group the store says nothing about keeps
    /// the default — the shape of a map written by toggles alone, which only ever holds the
    /// groups the user has touched.
    #[test]
    fn stored_fold_wins_and_missing_group_keeps_the_default() {
        let mut folds = std::collections::BTreeMap::new();
        folds.insert(InherentCategory::Basic, false);
        assert!(!folded(&folds, InherentCategory::Basic));
        assert!(folded(&folds, InherentCategory::Prestige));
        assert!(!folded(&folds, InherentCategory::Fitness));
    }

    /// The map round-trips through the JSON it is stored as, keyed by the category's lowercase
    /// names — the contract [`load`] reads back.
    #[test]
    fn the_map_round_trips_through_its_stored_json() {
        let mut folds = std::collections::BTreeMap::new();
        folds.insert(InherentCategory::Basic, false);
        folds.insert(InherentCategory::Granted, true);
        let json = serde_json::to_string(&folds).expect("a map of serializable keys");
        let back: std::collections::BTreeMap<InherentCategory, bool> =
            serde_json::from_str(&json).expect("the stored shape");
        assert_eq!(back, folds);
        assert!(json.contains("\"basic\":false"), "{json}");
    }
}
