//! The IO Sets tab's offer list — which sets the picker draws, in which order, and what each
//! facet button would land on.
//!
//! A pure function over the catalog because the alternative is a filter chain inlined in an
//! `rsx!` arm, where nothing can ask it a question. The facets cut across each other — a rarity
//! is not a slot category is not a piece count — so the interesting behaviour is in how they
//! COMPOSE, and composition is exactly what an inline chain hides. The counts matter for the same
//! reason: a facet button states how many sets it would leave, and a button that lands on an empty
//! list is worse than no button.

use crate::panels::stat_registry::{self, StatDef};
use coh_data::IoSet;

/// How the offered sets are ordered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SetSort {
    /// Alphabetical — the stable answer, and the one that survives a facet change.
    #[default]
    Name,
    /// By the level the set starts at, lowest first. What a levelling character is asking.
    Level,
}

/// The facets the sidebar sets. Every field is a narrowing; all-`None` offers the whole eligible
/// catalog.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SetFacets {
    /// The set's slotting category (`"Holds"`) — the export's `type`.
    pub set_type: Option<String>,
    /// The set's rarity tier (`"purple"`, `"ato"`) — the export's `category`.
    pub rarity: Option<String>,
    /// Piece count.
    pub size: Option<usize>,
    /// Only sets carrying at least one proc piece. The picker also hides those sets' non-proc
    /// pieces; this list only decides which sets appear.
    pub procs_only: bool,
    /// Only sets with a bonus to one of these tracked stat rows. `None` = off; on with nothing
    /// tracked narrows nothing, since a filter that empties the list would read as "no set fits".
    pub tracked_only: Option<Vec<String>>,
    /// Only sets with a bonus to this one stat row (a [`StatDef`] id) — the "which set here gives
    /// me X" question, asked without tracking X.
    pub bonus: Option<String>,
    pub sort: SetSort,
    /// Level-Up mode's ceiling: withhold sets the character could not craft yet. `None` = no
    /// gate. The set's own `min_level`, never a table.
    pub craftable_at: Option<i64>,
}

/// One set as the picker draws it.
pub struct SetOffer<'a> {
    pub id: &'a str,
    pub set: &'a IoSet,
    /// This set's piece count differs from the catalogue's usual one, so the row earns a size
    /// chip. The majority stays unmarked — the odd sizes are the only ink in a long list.
    pub off_size: bool,
}

/// One facet button: the value it selects, and how many sets it would leave.
pub struct FacetCount {
    pub value: String,
    pub count: usize,
}

impl SetFacets {
    /// Every narrowing EXCEPT `skip`, so a facet can count its own options against what the other
    /// facets have already left. Counting against the fully-filtered list would make every button
    /// but the active one read zero.
    fn admits(&self, set: &IoSet, skip: Facet) -> bool {
        if let Some(level) = self.craftable_at {
            if set.min_level > level {
                return false;
            }
        }
        if skip != Facet::SetType {
            if let Some(wanted) = &self.set_type {
                if &set.set_type != wanted {
                    return false;
                }
            }
        }
        if skip != Facet::Rarity {
            if let Some(wanted) = &self.rarity {
                if &set.category != wanted {
                    return false;
                }
            }
        }
        if skip != Facet::Size {
            if let Some(wanted) = self.size {
                if set.pieces.len() != wanted {
                    return false;
                }
            }
        }
        if self.procs_only && !set.pieces.iter().any(|piece| piece.proc) {
            return false;
        }
        if skip != Facet::Bonus {
            if let Some(id) = &self.bonus {
                if !crate::tracked_stats::has_tracked_bonus(set, std::slice::from_ref(id)) {
                    return false;
                }
            }
        }
        if skip != Facet::Tracked {
            if let Some(tracked) = &self.tracked_only {
                if !tracked.is_empty() && !crate::tracked_stats::has_tracked_bonus(set, tracked) {
                    return false;
                }
            }
        }
        true
    }
}

/// Which facet is being counted, and therefore not applied to its own tally.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Facet {
    SetType,
    Rarity,
    Size,
    Tracked,
    Bonus,
    None,
}

/// The sets to draw, narrowed by every facet and ordered by `sort`.
///
/// `usual_size` is the catalogue's majority piece count ([`coh_data::IoSetCatalog::most_common_piece_count`]);
/// `None` marks nothing, because with no majority there is nothing to be the exception to.
pub fn offered_sets<'a>(
    eligible: &[(&'a str, &'a IoSet)],
    facets: &SetFacets,
    usual_size: Option<usize>,
) -> Vec<SetOffer<'a>> {
    let mut offers: Vec<SetOffer<'a>> = eligible
        .iter()
        .filter(|(_, set)| facets.admits(set, Facet::None))
        .map(|(id, set)| SetOffer {
            id,
            set,
            off_size: usual_size.is_some_and(|usual| set.pieces.len() != usual),
        })
        .collect();
    match facets.sort {
        // Name is the tie-break under Level too, so the order is total either way — two sets
        // sharing a min level keep a stable position across re-renders.
        SetSort::Name => offers.sort_by(|a, b| a.set.name.cmp(&b.set.name)),
        SetSort::Level => offers.sort_by(|a, b| {
            a.set
                .min_level
                .cmp(&b.set.min_level)
                .then_with(|| a.set.name.cmp(&b.set.name))
        }),
    }
    offers
}

/// The slot categories present, with the count each would leave.
pub fn set_type_counts(eligible: &[(&str, &IoSet)], facets: &SetFacets) -> Vec<FacetCount> {
    tally(eligible, facets, Facet::SetType, |set| set.set_type.clone())
}

/// The rarity tiers present, with the count each would leave. Derived from the export's own
/// `category` vocabulary rather than a written list of tiers, so a fork that ships a seventh
/// rarity gets a button for it instead of having its sets fall out of the sidebar.
pub fn rarity_counts(eligible: &[(&str, &IoSet)], facets: &SetFacets) -> Vec<FacetCount> {
    tally(eligible, facets, Facet::Rarity, |set| set.category.clone())
}

/// The piece counts present, ascending, with the count each would leave.
pub fn size_counts(eligible: &[(&str, &IoSet)], facets: &SetFacets) -> Vec<(usize, usize)> {
    let mut counts: Vec<(usize, usize)> = tally(eligible, facets, Facet::Size, |set| {
        set.pieces.len().to_string()
    })
    .into_iter()
    .filter_map(|c| c.value.parse().ok().map(|size| (size, c.count)))
    .collect();
    counts.sort_unstable();
    counts
}

/// How many sets the tracked-stats option would leave, given the other facets. `None` when
/// nothing is tracked, so the button has no count to show rather than a misleading one.
pub fn tracked_count(
    eligible: &[(&str, &IoSet)],
    facets: &SetFacets,
    tracked: &[String],
) -> Option<usize> {
    if tracked.is_empty() {
        return None;
    }
    Some(
        eligible
            .iter()
            .filter(|(_, set)| facets.admits(set, Facet::Tracked))
            .filter(|(_, set)| crate::tracked_stats::has_tracked_bonus(set, tracked))
            .count(),
    )
}

/// One bonus the offered sets grant, with how many of them grant it.
#[derive(Clone)]
pub struct BonusCount {
    pub stat: &'static StatDef,
    pub count: usize,
}

// By id: the registry's rows are statics, so the id is the row.
impl PartialEq for BonusCount {
    fn eq(&self, other: &Self) -> bool {
        self.stat.id == other.stat.id && self.count == other.count
    }
}

/// Every stat row some offered set has a bonus to, in the dashboard's own order, with the count
/// each would leave. The chosen row is kept at zero if the other facets have filtered it out,
/// so the dropdown can still show what is selected.
pub fn bonus_counts(eligible: &[(&str, &IoSet)], facets: &SetFacets) -> Vec<BonusCount> {
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (_, set) in eligible
        .iter()
        .filter(|(_, set)| facets.admits(set, Facet::Bonus))
    {
        let mut rows: Vec<&'static str> = set
            .bonuses
            .iter()
            .flat_map(|tier| &tier.effects)
            .flat_map(crate::tracked_stats::rows_for_effect)
            .collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            *counts.entry(row).or_default() += 1;
        }
    }
    stat_registry::ALL
        .iter()
        .filter_map(|stat| {
            let count = counts.get(stat.id).copied();
            let chosen = facets.bonus.as_deref() == Some(stat.id);
            (count.is_some() || chosen).then(|| BonusCount {
                stat,
                count: count.unwrap_or(0),
            })
        })
        .collect()
}

/// How many sets survive every facet — what an "any" button reports.
pub fn total_offered(eligible: &[(&str, &IoSet)], facets: &SetFacets) -> usize {
    eligible
        .iter()
        .filter(|(_, set)| facets.admits(set, Facet::None))
        .count()
}

fn tally(
    eligible: &[(&str, &IoSet)],
    facets: &SetFacets,
    facet: Facet,
    key: impl Fn(&IoSet) -> String,
) -> Vec<FacetCount> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (_, set) in eligible.iter().filter(|(_, set)| facets.admits(set, facet)) {
        *counts.entry(key(set)).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(value, count)| FacetCount { value, count })
        .collect()
}

/// The human label for a rarity token. A display label, not a branch — the token is what the
/// filter keys on (Rule 0); this only decides what the button reads. An unrecognized token is
/// title-cased rather than dropped, so a fork's own vocabulary reaches the user as itself instead
/// of vanishing from the sidebar.
pub fn rarity_label(category: &str) -> String {
    match category {
        "purple" => "Very Rare".to_string(),
        "ato" => "Archetype".to_string(),
        "pvp" => "PvP".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{IoSetCatalog, PowerDatabase};
    use std::path::PathBuf;

    fn catalog() -> IoSetCatalog {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contract/homecoming/bundle.json.gz");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        PowerDatabase::from_gz_bytes(&bytes)
            .expect("bundle loads")
            .io_sets
            .expect("catalog present")
    }

    /// The chip's count is what pressing it leaves, including alongside another facet.
    #[test]
    fn the_tracked_chip_counts_what_it_leaves() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let tracked = vec!["res_energy".to_string(), "res_negative".to_string()];
        for rarity in [None, Some("rare".to_string())] {
            let off = SetFacets {
                rarity: rarity.clone(),
                ..SetFacets::default()
            };
            let on = SetFacets {
                tracked_only: Some(tracked.clone()),
                ..off.clone()
            };
            let count = tracked_count(&eligible, &off, &tracked).expect("something tracked");
            let left = offered_sets(&eligible, &on, None);
            assert_eq!(count, left.len(), "rarity {rarity:?}");
            assert!(count > 0 && count < total_offered(&eligible, &off));
            assert!(left
                .iter()
                .all(|offer| crate::tracked_stats::has_tracked_bonus(offer.set, &tracked)));
        }
    }

    /// Every count in the bonus dropdown is what choosing that bonus leaves, with another facet on.
    #[test]
    fn each_bonus_count_is_what_choosing_it_leaves() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let base = SetFacets {
            rarity: Some("rare".to_string()),
            ..SetFacets::default()
        };
        let counts = bonus_counts(&eligible, &base);
        assert!(counts.iter().any(|c| c.stat.id == "res_negative"));
        for row in &counts {
            let chosen = SetFacets {
                bonus: Some(row.stat.id.to_string()),
                ..base.clone()
            };
            assert_eq!(
                offered_sets(&eligible, &chosen, None).len(),
                row.count,
                "{}",
                row.stat.id
            );
        }
    }

    /// A chosen bonus the other facets have filtered out stays in the list, at zero.
    #[test]
    fn a_chosen_bonus_stays_listed_at_zero() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let facets = SetFacets {
            bonus: Some("res_negative".to_string()),
            size: Some(1),
            ..SetFacets::default()
        };
        let counts = bonus_counts(&eligible, &facets);
        let chosen = counts.iter().find(|c| c.stat.id == "res_negative");
        assert_eq!(chosen.map(|c| c.count), Some(0));
    }

    /// Switched on with nothing tracked, the option narrows nothing and has no count.
    #[test]
    fn the_tracked_chip_with_nothing_tracked_keeps_every_set() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let on = SetFacets {
            tracked_only: Some(Vec::new()),
            ..SetFacets::default()
        };
        assert_eq!(total_offered(&eligible, &on), eligible.len());
        assert_eq!(tracked_count(&eligible, &on, &[]), None);
    }
}
