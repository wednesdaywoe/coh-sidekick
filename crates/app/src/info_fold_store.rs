//! Which sections of the Info card are open and which are folded, kept across reloads on the
//! same `localStorage` channel as [`crate::inherent_fold_store`].
//!
//! **A preference, not build state** — the same argument [`crate::inherent_fold_store`] makes
//! for the inherent panels: folding is a statement about the reader, not about the power or
//! the build. An experienced player who never reads the Accepts chips wants that once, for
//! every power and every surface, not per card; and putting it on
//! [`coh_data::CharacterState`] would make one reader's folded sections travel to every other
//! reader of a shared build.
//!
//! **One preference overall, not one per power** (decided 2026-11-24, owner): the sections
//! repeat across every power on the card — Damage here, Effects there, the same tier tables
//! on every Details tab — and the question "do I read mez tables?" has one answer. Folding
//! per power would spend memory re-asking it for every card.
//!
//! **Seeded with the authored defaults.** A section the store says nothing about keeps the
//! answer [`default_folded`] gives — Damage, Effects and Cost and timing open, everything
//! else folded — so a fresh profile opens on the damage-first card the sprawl work asked
//! for, and the first fold or expand writes the memory.
//!
//! **Keys are authored constants, not derived from prose.** `section` names every foldable
//! section once; the only derived keys are the execution-group and effect-section ids, which
//! slugify headings this crate authors ([`section::execution`], [`section::effects`]). A
//! renamed heading silently resets that one section's memory to the default — a real cost of
//! derived keys, and the reason the fixed ones are not derived.

use dioxus::prelude::*;

const KEY_INFO_FOLDS: &str = "sk-info-folds";

/// The stable id of every foldable section of the Info card.
///
/// Namespaced by tab (`info:` / `details:`) because the two tabs carry sections with the
/// same heading — both say "Damage" — and they are different content: the meter on one, the
/// tier table on the other. A player who folds one has not said anything about the other.
pub mod section {
    /// The hit: hero number, bar, component rows. A summon's henchmen fold with it — the
    /// roster is that power's damage, drawn where the Damage block sits.
    pub const DAMAGE: &str = "info:damage";
    /// The power's headline effects on the Info tab.
    pub const EFFECTS: &str = "info:effects";
    pub const SLOTTED_PROCS: &str = "info:slotted-procs";
    pub const PERMA: &str = "info:perma";

    /// The tier-by-tier damage table on the Details tab.
    pub const DAMAGE_TIERS: &str = "details:damage-tiers";
    pub const ENHANCEMENT_DIVERSIFICATION: &str = "details:enhancement-diversification";
    pub const ENHANCEMENT_BONUSES: &str = "details:enhancement-bonuses";
    pub const ACCEPTS: &str = "details:accepts";

    /// One of the Info tab's execution groups, keyed by its authored heading
    /// (`Cost and timing`, `Reach`, `Other`).
    pub fn execution(title: &str) -> String {
        namespaced("info", title)
    }

    /// One of the Details tab's effect sections, keyed by its authored heading (`Mez`,
    /// `Buffs`, `Debuffs`, `Damage`, `Special`).
    pub fn effects(title: &str) -> String {
        namespaced("details", title)
    }

    fn namespaced(tab: &str, title: &str) -> String {
        format!("{tab}:{}", title.to_lowercase().replace(' ', "-"))
    }
}

/// The sections a card shows before the reader has touched a fold: the number the power is
/// picked for, what it does, and what it costs. Everything a veteran of the formulas reads
/// from memory starts folded (owner, 2026-11-24: most players "are mostly just interested in
/// seeing the damage meter"). The rest of the card is one click away, and the click sticks.
///
/// `Cost and timing` is spelled here rather than derived, and a test pins it to
/// [`section::execution`] — if the heading's slug ever moves, this list moves with it or
/// the default card loses a section it promised to show.
const OPEN_BY_DEFAULT: &[&str] = &[section::DAMAGE, section::EFFECTS, "info:cost-and-timing"];

/// Whether a section is folded when nothing is stored.
pub fn default_folded(id: &str) -> bool {
    !OPEN_BY_DEFAULT.contains(&id)
}

/// The fold state, provided once at the shell root. `true` = folded. A section missing from
/// the map is on its authored default.
#[derive(Clone, Copy)]
pub struct InfoFolds(pub Signal<std::collections::BTreeMap<String, bool>>);

/// The fold for one section: the stored answer when there is one, the default when not.
pub fn folded(folds: &std::collections::BTreeMap<String, bool>, id: &str) -> bool {
    folds.get(id).copied().unwrap_or_else(|| default_folded(id))
}

/// One section's fold, read and written through the shared persisted map.
///
/// Unlike [`crate::inherent_fold_store::InherentFold`] this is not `Copy` — the id is a
/// `String`, not a variant — which is why it travels behind an `Option` by reference like
/// that one does. `PartialEq` because the component memoizes props with it.
#[derive(Clone, PartialEq)]
pub struct InfoFold {
    folds: Signal<std::collections::BTreeMap<String, bool>>,
    id: String,
}

impl InfoFold {
    pub fn new(folds: Signal<std::collections::BTreeMap<String, bool>>, id: String) -> Self {
        Self { folds, id }
    }

    /// Whether the section's body is folded right now.
    pub fn folded(&self) -> bool {
        folded(&self.folds.read(), &self.id)
    }

    /// Fold the body, or open it, whichever it is not. The write lands in the map;
    /// persisting it is the shell's effect, like every other preference.
    pub fn toggle(mut self) {
        let next = !self.folded();
        self.folds.write().insert(self.id.clone(), next);
    }
}

/// Persist the map (fire-and-forget, like [`crate::inherent_fold_store::persist`]).
///
/// Written as one JSON object keyed by the section ids. Anything unreadable on the way back
/// in is treated as nothing stored — the defaults are the honest answer, and a corrupt key
/// must not fold sections the user left open.
///
/// An empty map stores nothing: the map only ever grows (a toggle inserts, nothing deletes),
/// so empty means the reader has not folded or opened a single section yet — and the start-up
/// restore, which reads the stored map back into the signal, would otherwise write the
/// defaults to storage on every fresh profile. A key the app never wrote is worth more than
/// a key saying `{}`: the first is the authored default, the second is one stale answer away
/// from overriding a future one.
pub fn persist(folds: &std::collections::BTreeMap<String, bool>) {
    if folds.is_empty() {
        return;
    }
    let Ok(json) = serde_json::to_string(folds) else {
        return;
    };
    let js =
        format!("try {{ localStorage.setItem({KEY_INFO_FOLDS:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

/// The persisted map; `None` (missing, or anything we didn't write) keeps the defaults.
pub async fn load() -> Option<std::collections::BTreeMap<String, bool>> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_INFO_FOLDS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    let text = value.as_str()?;
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The damage-first card: Damage, Effects and Cost and timing open, everything else —
    /// Reach, procs, perma, the whole Details tab — folded. If this drifts, a fresh profile
    /// changes shape with no store behind it.
    #[test]
    fn only_damage_effects_and_cost_and_timing_open_the_card() {
        let open = [
            section::DAMAGE,
            section::EFFECTS,
            "info:cost-and-timing",
            "info:reach",
            "info:other",
            section::SLOTTED_PROCS,
            section::PERMA,
            section::DAMAGE_TIERS,
            &section::effects("Mez"),
            &section::effects("Buffs"),
            &section::effects("Debuffs"),
            &section::effects("Damage"),
            &section::effects("Special"),
            section::ENHANCEMENT_DIVERSIFICATION,
            section::ENHANCEMENT_BONUSES,
            section::ACCEPTS,
        ];
        let open: Vec<_> = open.into_iter().filter(|id| !default_folded(id)).collect();
        assert_eq!(
            open,
            vec![section::DAMAGE, section::EFFECTS, "info:cost-and-timing"]
        );
    }

    /// The default list's one derived-looking key IS the derived key for the heading the
    /// Info tab draws. A rename of `Cost and timing` that moves the slug must red here,
    /// not quietly fold a section the fresh card promised to show.
    #[test]
    fn the_cost_and_timing_key_matches_its_derived_key() {
        assert_eq!(
            section::execution("Cost and timing"),
            "info:cost-and-timing",
            "the open-by-default key and the slug must agree"
        );
    }

    /// A stored answer wins over the default, and a section the store says nothing about
    /// keeps the default — the shape of a map written by toggles alone.
    #[test]
    fn stored_fold_wins_and_missing_section_keeps_the_default() {
        let mut folds = std::collections::BTreeMap::new();
        folds.insert(section::DAMAGE.to_string(), true);
        folds.insert(section::ACCEPTS.to_string(), false);
        assert!(folded(&folds, section::DAMAGE));
        assert!(!folded(&folds, section::ACCEPTS));
        assert!(!folded(&folds, section::EFFECTS));
        assert!(folded(&folds, section::PERMA));
    }

    /// The map round-trips through the JSON it is stored as — the contract [`load`] reads
    /// back.
    #[test]
    fn the_map_round_trips_through_its_stored_json() {
        let mut folds = std::collections::BTreeMap::new();
        folds.insert(section::PERMA.to_string(), false);
        folds.insert(section::effects("Mez"), true);
        let json = serde_json::to_string(&folds).expect("a map of string keys");
        let back: std::collections::BTreeMap<String, bool> =
            serde_json::from_str(&json).expect("the stored shape");
        assert_eq!(back, folds);
        assert!(
            json.contains("\"details:mez\":true"),
            "tab-namespaced keys must survive: {json}"
        );
    }

    /// The two tabs' "Damage" sections are different keys — folding the meter does not fold
    /// the tier table, and vice versa.
    #[test]
    fn the_two_damage_sections_are_separate_keys() {
        assert_ne!(section::DAMAGE, section::DAMAGE_TIERS);
        assert_ne!(section::DAMAGE, section::effects("Damage"));
        assert_ne!(section::DAMAGE_TIERS, section::effects("Damage"));
    }
}
