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
    /// By rarity, the common end at the top: the ladder read upwards, as the game's own
    /// rarity rungs are numbered.
    RarityCommonFirst,
    /// By rarity, the rare end at the top. The same chunks in the other order — what is inside
    /// a chunk keeps reading the same way, because a chunk is a level run either way.
    RarityRareFirst,
}

/// The seat of a rarity the export names that this build does not know. It is deliberately the
/// last seat in EITHER direction: an unrecognised tier is unranked, not the scarcest thing in
/// the catalogue, and filing a fork's own vocabulary at the top of a "rarest first" list would
/// say something about it nobody has checked.
const RARITY_UNRANKED: u8 = u8::MAX;

impl SetSort {
    /// The token the sort dropdown carries. Kept here so the option list and the `onchange`
    /// branch cannot drift into disagreeing about what a value means.
    pub fn token(self) -> &'static str {
        match self {
            SetSort::Name => "name",
            SetSort::Level => "level",
            SetSort::RarityCommonFirst => "rarity",
            SetSort::RarityRareFirst => "rarity-rare",
        }
    }

    /// The sort a token names. Anything unrecognised is [`SetSort::Name`], the default, rather
    /// than a guess at which of the two rarity directions was meant.
    pub fn from_token(token: &str) -> Self {
        match token {
            "level" => SetSort::Level,
            "rarity" => SetSort::RarityCommonFirst,
            "rarity-rare" => SetSort::RarityRareFirst,
            _ => SetSort::Name,
        }
    }

    /// Whether the list is ordered by rarity, which is what decides whether it is chunked.
    pub fn by_rarity(self) -> bool {
        matches!(self, SetSort::RarityCommonFirst | SetSort::RarityRareFirst)
    }
}

/// Where a rarity sits on the ladder, common end first.
///
/// The game's three rungs come first — Uncommon, Rare and Very Rare are what an item tooltip
/// actually states — and the three tiers that name how a set is ACQUIRED rather than how scarce
/// it is come after them, in the order the beta's "all sets" view has always filed them
/// (`specialSortOrder`: standard sets, then purple, then PvP, then ATO). The tokens are the
/// export's own `category` vocabulary, the same one [`rarity_label`] reads.
fn rarity_rank(category: &str) -> u8 {
    match category {
        "uncommon" => 0,
        "rare" => 1,
        "purple" => 2,
        "event" => 3,
        "pvp" => 4,
        "ato" => 5,
        _ => RARITY_UNRANKED,
    }
}

/// Two rarities against each other, `rarest_first` deciding which end wins. An unranked tier
/// loses to everything in both directions, so it sits at the bottom of the list whichever way
/// the ladder is read.
fn rarity_order(left: &str, right: &str, rarest_first: bool) -> std::cmp::Ordering {
    let (l, r) = (rarity_rank(left), rarity_rank(right));
    match (l == RARITY_UNRANKED, r == RARITY_UNRANKED) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) if rarest_first => r.cmp(&l),
        (false, false) => l.cmp(&r),
    }
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
        // The two rarity orders differ ONLY in which end of the ladder is at the top. Inside a
        // chunk the reading is the Level order — level up, then name — because a chunk is a run
        // of levels either way, and reversing it too would make "rarest first" unreadable as
        // anything but a mirror.
        sort @ (SetSort::RarityCommonFirst | SetSort::RarityRareFirst) => {
            let rarest_first = sort == SetSort::RarityRareFirst;
            offers.sort_by(|a, b| {
                rarity_order(&a.set.category, &b.set.category, rarest_first)
                    .then_with(|| a.set.min_level.cmp(&b.set.min_level))
                    .then_with(|| a.set.name.cmp(&b.set.name))
            });
        }
    }
    offers
}

/// The line the list opens each rarity chunk with, positionally: `Some("Uncommon (63)")` on the
/// first offer of a run and `None` on every row after it, so the chunks read as chunks instead
/// of as an alphabetical list that has somehow come apart.
///
/// Empty — one `None` per row — in two cases. When the list is not ordered by rarity, because
/// then the rows are not grouped by anything the header could claim. And when every row is the
/// same rarity, which is what the rarity filter leaves: a heading repeating the filter one line
/// below it is ink that says nothing, on the same reasoning that leaves a one-option dropdown
/// undrawn.
pub fn chunk_headers(offers: &[SetOffer<'_>], sort: SetSort) -> Vec<Option<String>> {
    if !sort.by_rarity() {
        return vec![None; offers.len()];
    }
    // A run starts wherever the rarity differs from the row above. The list is already sorted,
    // so every run is contiguous and one pass sees them all.
    let mut starts: Vec<bool> = Vec::with_capacity(offers.len());
    let mut previous: Option<&str> = None;
    for offer in offers {
        let category = offer.set.category.as_str();
        starts.push(previous != Some(category));
        previous = Some(category);
    }
    if starts.iter().filter(|started| **started).count() < 2 {
        return vec![None; offers.len()];
    }
    let mut headers: Vec<Option<String>> = vec![None; offers.len()];
    for (index, started) in starts.iter().enumerate() {
        if !started {
            continue;
        }
        // The run runs to the next start, or to the end of the list.
        let end = starts[index + 1..]
            .iter()
            .position(|later| *later)
            .map_or(offers.len(), |ahead| index + 1 + ahead);
        headers[index] = Some(format!(
            "{} ({})",
            rarity_label(&offers[index].set.category),
            end - index
        ));
    }
    headers
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
    use coh_data::{IoSetCatalog, IoSetPiece, PowerDatabase};
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

    /// The whole catalogue ordered by rarity, at both ends of the ladder: the two lists a user
    /// gets from the two dropdown entries, side by side.
    fn both_ends<'a>(eligible: &[(&'a str, &'a IoSet)]) -> (Vec<SetOffer<'a>>, Vec<SetOffer<'a>>) {
        (
            offered_sets(
                eligible,
                &SetFacets {
                    sort: SetSort::RarityCommonFirst,
                    ..SetFacets::default()
                },
                None,
            ),
            offered_sets(
                eligible,
                &SetFacets {
                    sort: SetSort::RarityRareFirst,
                    ..SetFacets::default()
                },
                None,
            ),
        )
    }

    /// The rarities a list runs through, one entry per run — so a rarity that appears in two
    /// separate runs shows up twice, which is exactly the failure worth catching.
    fn runs<'a>(offers: &'a [SetOffer<'a>]) -> Vec<&'a str> {
        let mut seen: Vec<&str> = Vec::new();
        for offer in offers {
            if seen.last() != Some(&offer.set.category.as_str()) {
                seen.push(&offer.set.category);
            }
        }
        seen
    }

    /// The ladder, on the shipped catalogue: every rarity in one contiguous run, the game's
    /// three rungs first and the three acquisition tiers after them.
    #[test]
    fn the_rarity_ladder_groups_every_rarity_into_one_run() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let (common, rare) = both_ends(&eligible);
        assert_eq!(common.len(), eligible.len(), "a sort loses a set");
        assert_eq!(
            runs(&common),
            ["uncommon", "rare", "purple", "event", "pvp", "ato"]
        );
        // Rarest first is the same runs end for end, not a different grouping of the list.
        let down = runs(&rare);
        assert_eq!(down.len(), runs(&common).len());
        assert!(down.iter().eq(runs(&common).iter().rev()));
        // One run per rarity means each run holds everything that rarity has — which is the
        // count the rarity dropdown already states for it.
        for facet in rarity_counts(&eligible, &SetFacets::default()) {
            assert_eq!(
                common
                    .iter()
                    .filter(|offer| offer.set.category == facet.value)
                    .count(),
                facet.count,
                "{}",
                facet.value
            );
        }
    }

    /// Inside a chunk the reading is the level order, at both ends of the ladder: a chunk is a
    /// run of levels whichever rarity sits on top, so flipping the ladder must not scramble it.
    #[test]
    fn each_rarity_chunk_reads_in_level_order() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let (common, rare) = both_ends(&eligible);
        for offers in [&common, &rare] {
            for chunk in offers.chunk_by(|a, b| a.set.category == b.set.category) {
                for (ahead, behind) in chunk.iter().zip(chunk.iter().skip(1)) {
                    let ahead_key = (ahead.set.min_level, ahead.set.name.as_str());
                    let behind_key = (behind.set.min_level, behind.set.name.as_str());
                    assert!(
                        ahead_key <= behind_key,
                        "{}: {:?} above {:?}",
                        ahead.set.category,
                        ahead_key,
                        behind_key
                    );
                }
            }
        }
    }

    /// Each run is named once, above its own sets, and says how many sets it holds — the same
    /// number the rarity dropdown shows for that tier.
    #[test]
    fn a_header_names_each_rarity_run_with_its_size() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let (common, _) = both_ends(&eligible);
        let headers = chunk_headers(&common, SetSort::RarityCommonFirst);
        assert_eq!(headers.len(), common.len());
        let mut named: Vec<&str> = headers.iter().flatten().map(String::as_str).collect();
        let expected: Vec<String> = ["uncommon", "rare", "purple", "event", "pvp", "ato"]
            .into_iter()
            .map(|category| {
                let count = rarity_counts(&eligible, &SetFacets::default())
                    .into_iter()
                    .find(|facet| facet.value == category)
                    .unwrap_or_else(|| panic!("{category} is in the list but not the tally"))
                    .count;
                format!("{} ({})", rarity_label(category), count)
            })
            .collect();
        assert_eq!(named, expected);
        // And the header sits on the row that opens its run, not merely somewhere in it.
        named.clear();
        for (offer, header) in common.iter().zip(&headers) {
            if header.is_some() {
                named.push(offer.set.category.as_str());
            }
        }
        assert_eq!(named, runs(&common));
    }

    /// Filtered down to a single rarity there is nothing to chunk, and a heading repeating the
    /// dropdown one line above is ink that says nothing.
    #[test]
    fn one_rarity_gets_no_header() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        let facets = SetFacets {
            rarity: Some("purple".to_string()),
            sort: SetSort::RarityRareFirst,
            ..SetFacets::default()
        };
        let offers = offered_sets(&eligible, &facets, None);
        assert!(!offers.is_empty(), "the fixture rarity has vanished");
        assert!(chunk_headers(&offers, facets.sort)
            .iter()
            .all(Option::is_none));
    }

    /// A list not ordered by rarity is not grouped by it either, so it gets no headings —
    /// which is how the list stays exactly as it was for the two sorts that came first.
    #[test]
    fn a_list_not_ordered_by_rarity_is_not_chunked() {
        let catalog = catalog();
        let eligible: Vec<(&str, &IoSet)> = catalog
            .sets
            .iter()
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        for sort in [SetSort::Name, SetSort::Level] {
            let offers = offered_sets(
                &eligible,
                &SetFacets {
                    sort,
                    ..SetFacets::default()
                },
                None,
            );
            assert_eq!(offers.len(), eligible.len());
            assert!(
                chunk_headers(&offers, sort).iter().all(Option::is_none),
                "{sort:?}"
            );
        }
    }

    /// A tier this build has never seen is UNRANKED, not the rarest thing in the catalogue, so
    /// it keeps the last seat whichever end of the ladder is read first.
    #[test]
    fn a_rarity_this_build_does_not_know_sits_last_either_way() {
        let sets = [
            ("a", hand_set("Ambered Ward", "glacial", 20)),
            ("b", hand_set("Bright Ward", "rare", 30)),
            ("c", hand_set("Cinder Ward", "uncommon", 10)),
        ];
        let eligible: Vec<(&str, &IoSet)> = sets.iter().map(|(id, set)| (*id, set)).collect();
        for (sort, want) in [
            (SetSort::RarityCommonFirst, ["uncommon", "rare", "glacial"]),
            (SetSort::RarityRareFirst, ["rare", "uncommon", "glacial"]),
        ] {
            let offers = offered_sets(
                &eligible,
                &SetFacets {
                    sort,
                    ..SetFacets::default()
                },
                None,
            );
            let got: Vec<&str> = offers
                .iter()
                .map(|offer| offer.set.category.as_str())
                .collect();
            assert_eq!(got, want.to_vec(), "{sort:?}");
        }
    }

    /// The dropdown's option list and its `onchange` branch cannot disagree about a value,
    /// because both read these two functions.
    #[test]
    fn every_sort_token_names_its_sort_and_back() {
        for sort in [
            SetSort::Name,
            SetSort::Level,
            SetSort::RarityCommonFirst,
            SetSort::RarityRareFirst,
        ] {
            assert_eq!(SetSort::from_token(sort.token()), sort, "{}", sort.token());
        }
        // The "any" seat and a value from a future build both land on the default rather than
        // on a guess at which rarity direction was meant.
        assert_eq!(SetSort::from_token(""), SetSort::Name);
        assert_eq!(SetSort::from_token("rarity-rarest"), SetSort::Name);
    }

    /// A set built by hand, for the cases the shipped catalogues cannot make: a rarity no
    /// dataset ships, and a list short enough to assert an exact order against.
    fn hand_set(name: &str, category: &str, min_level: i64) -> IoSet {
        IoSet {
            name: name.to_string(),
            icon: String::new(),
            category: category.to_string(),
            rarity: String::new(),
            set_type: "Holds".to_string(),
            min_level,
            max_level: min_level + 10,
            attuned_only: false,
            bin_order: None,
            pieces: vec![IoSetPiece {
                num: 1,
                name: "Piece".to_string(),
                aspects: vec![],
                proc: false,
                total_aspects: None,
                unique: false,
            }],
            bonuses: vec![],
        }
    }
}
