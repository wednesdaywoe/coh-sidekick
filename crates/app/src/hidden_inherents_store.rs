//! Which granted inherents the user has hidden, kept across reloads on the same `localStorage`
//! channel as [`crate::slot_level_store`].
//!
//! **A preference, not build state.** Hiding a power is a statement about the Powers window —
//! "I never look at Brawl" — and it applies to every build the user opens, the way the theme
//! does. Putting it on [`coh_data::CharacterState`] would make it travel with a shared build,
//! so one player's tidy window would hide cards in every reader's; and it would make each
//! toggle an undo step in a history that is about the build.
//!
//! **Hiding never changes what a power contributes.** The power stays in the build, keeps its
//! slotted enhancements and keeps feeding the totals; only the card is not drawn. That is why
//! the panel draws a footer naming what it hides: a hidden power that is switched ON is still
//! contributing with no control on screen to switch it off, and the footer is the one trace
//! that it is there.
//!
//! **Only powers that take no slots are offered it.** A slotted power that vanished would take
//! its slotting out of sight while still spending the build's slots — the §"one home" problem
//! `panels::dashboards` solved for stats, in its worst form. The offer is made per power from
//! the definition ([`coh_data::Power::accepts_enhancements`]), never from a name list, so a
//! fork that marks a power differently gets the right answer with no edit here.
//!
//! Keyed by the power's internal name and nothing else: granted inherents carry the synthetic
//! `Inherent` set rather than a real owning one, and their names are unique across the whole
//! corpus — the same property [`coh_data::PowerDatabase::find_granted_power`] relies on.

use dioxus::prelude::*;

const KEY_HIDDEN_INHERENTS: &str = "sk-hidden-inherents";

/// The hidden set, provided once at the shell root. A `Signal<BTreeSet<String>>` rather than a
/// newtype over it, like [`crate::panels::powers::ShowSlotLevels`] wraps its bool: one field,
/// and no method that could hold a second copy of the answer.
#[derive(Clone, Copy)]
pub struct HiddenInherents(pub Signal<std::collections::BTreeSet<String>>);

/// Whether this power's card is drawn. A power with nothing in the set is drawn, which is the
/// state a fresh profile is in.
pub fn is_hidden(hidden: &std::collections::BTreeSet<String>, internal_name: &str) -> bool {
    hidden.contains(internal_name)
}

/// Persist the set (fire-and-forget, like [`crate::level_up_store::persist`]).
///
/// Written as one JSON array of names. Anything unreadable on the way back in is treated as
/// nothing hidden — the failure has to be visible rather than the alternative, which is a card
/// the user cannot find and no state saying why.
pub fn persist(hidden: &std::collections::BTreeSet<String>) {
    let names: Vec<&String> = hidden.iter().collect();
    let Ok(json) = serde_json::to_string(&names) else {
        return;
    };
    let js = format!(
        "try {{ localStorage.setItem({KEY_HIDDEN_INHERENTS:?}, {json:?}); }} catch (_) {{}}"
    );
    crate::storage::commit(js);
}

/// The persisted set; `None` (missing, or anything we didn't write) hides nothing.
pub async fn load() -> Option<std::collections::BTreeSet<String>> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_HIDDEN_INHERENTS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    let text = value.as_str()?;
    let names: Vec<String> = serde_json::from_str(text).ok()?;
    Some(names.into_iter().collect())
}
