//! The set-bonus finder (the beta `SetBonusLookupModal`): every bonus in the dataset's own IO-set
//! catalog, searchable by the effect it grants rather than by the set that grants it.
//!
//! The planner already answers "what do these sets give me" — the slot tooltip shows a set's tiers
//! while slotting, and the detailed sheet shows which bonuses made a number. This answers the
//! question a build is actually planned around, which runs the other way: *what would give me
//! more recharge*, before anything is slotted.
//!
//! # The hierarchy is the dashboard's, not a table of its own
//!
//! The beta reached its Effect → Type cascade through two hand-written maps (`NORMALIZED_EFFECT_MAP`,
//! `RAW_STAT_EFFECT_MAP`) plus prefix guesses, and any stat all three missed fell out of the index
//! silently — a set bonus missing from a search reads exactly like a set that does not grant it.
//!
//! Here both levels are read off vocabulary that already exists and is already gated:
//!
//! - **Effect** is the [`StatSection`] — the dashboard panel the bonus would move.
//! - **Type** is the [`StatDef`] row inside it, via [`stat_registry::rows_for_bonus_stat`].
//!
//! So a bonus is filed under the row that will change when it is slotted, and the finder inherits
//! from the registry it borrows the property that matters most: the mapping is TOTAL — `every_routed_set_bonus_stat_reaches_some_dashboard_row` proves every
//! stat reaches a row, so nothing can drop out unremarked. What the vocabulary genuinely cannot
//! place ([`set_bonuses::NamedStats`]'s three fault variants) is listed as a fault, not
//! swallowed.
//!
//! # The index is the set-bonus catalog, and only that
//!
//! The index is built from [`set_bonuses::stats_named`] over the catalog's tiers; nothing in this
//! module contributes to a total. A unique PIECE's global (Steadfast's +3% Def, LotG's +Recharge)
//! is not a set bonus and is not here — the legacy data's pseudo-tiers that made them findable
//! were removed as a double-count (DATA-GAP-REGISTER MEZRES-1 residue 2), and the recorded exit
//! for the lost discoverability is indexing the proc database's globals beside these tiers.

use crate::build_session::BuildSession;
use crate::floating::FloatingPanel;
use crate::panels::powers::format_bonus_desc;
use crate::panels::stat_registry::{self, StatDef, StatSection};
use crate::shell::Db;
use coh_data::{CharacterState, IoSetCatalog, PowerDatabase};
use coh_math::set_bonuses::{self, NamedStats};
use dioxus::prelude::*;
use std::collections::BTreeSet;

// ============================================================
// The index.
// ============================================================

/// One searchable bonus: a tier of a set, seen through one of the dashboard rows it feeds.
///
/// A tier that grants several effects, or one effect that expands (`damage_resistance_(all)`),
/// yields one entry per row — the unit of the search is the effect a reader is looking for, not
/// the tier that happens to carry it.
#[derive(Clone, PartialEq, Debug)]
pub struct LookupEntry {
    pub set_name: String,
    /// The set's slotting category (`"Ranged Damage"`, `"Holds"`) — what powers can take it.
    pub set_type: String,
    /// The display rarity tier (`purple`, `ato`, `rare`, …), shown as a plain chip. Deliberately
    /// not a colour code: this app already spends hue on the stat families and the incarnate
    /// rarity tiers, and a third scheme in a table that also carries a stat row would be two
    /// codes competing in one line.
    pub category: String,
    pub min_level: i64,
    pub max_level: i64,
    /// How many pieces of the set must be slotted in one power for this tier to fire.
    pub pieces: u8,
    /// The dashboard row this bonus lands on — its section is the Effect, its label the Type.
    pub row: &'static StatDef,
    pub value: f64,
    /// PvP-zone-only, which the calc skips entirely in PvE. Shown and marked rather than
    /// filtered: a bonus that only works in one place is a fact worth reading before slotting.
    pub pvp: bool,
    /// The export's own phrasing, with the precise value spliced in — shown only when
    /// [`expanded`](Self::expanded) says the row label is not the whole story.
    pub description: String,
    /// This effect was filed under more than one row: an `(all)` label, a paired label such as
    /// Smashing/Lethal, or the recharge-debuff bonus the engine fans into Slow as well. Those are the rows whose label alone misleads —
    /// Steadfast under "Melee" is `+3% All Defense`, not a melee-specific bonus — so they show
    /// the export's own phrasing beneath. A bonus that names exactly one row does not: there,
    /// the description says "+15% Accuracy" under a row already labelled Accuracy.
    pub expanded: bool,
}

/// A bonus the vocabulary could not file. Shown in the modal rather than dropped, because the
/// alternative is a catalog that quietly under-reports (Rule 1).
#[derive(Clone, PartialEq, Debug)]
pub struct IndexFault {
    pub set_name: String,
    pub pieces: u8,
    pub detail: String,
}

/// The whole catalog, indexed once per dataset.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LookupIndex {
    pub entries: Vec<LookupEntry>,
    pub faults: Vec<IndexFault>,
}

/// Walk every set, every tier, every effect, and file each under the dashboard rows it names.
///
/// Deduplicated on `(set, tier, row, value)`, so a stat the export names twice in one tier is
/// listed once under its row.
/// Pure over the catalog — the sort is stable and the map is ordered, so the index is identical
/// run to run.
pub fn build_index(catalog: &IoSetCatalog) -> LookupIndex {
    let mut index = LookupIndex::default();
    let mut seen: BTreeSet<(String, u8, &'static str, u64)> = BTreeSet::new();

    for (set_id, set) in &catalog.sets {
        for tier in &set.bonuses {
            for effect in &tier.effects {
                let mut rows = match set_bonuses::stats_named(effect) {
                    NamedStats::Stats(stats) => stats
                        .into_iter()
                        .flat_map(stat_registry::rows_for_bonus_stat)
                        .collect::<Vec<_>>(),
                    NamedStats::UntypedMezAll => {
                        index.faults.push(IndexFault {
                            set_name: set.name.clone(),
                            pieces: tier.pieces,
                            detail: "mez resistance to \"all\", with no types named in the export"
                                .to_string(),
                        });
                        continue;
                    }
                    NamedStats::UnknownMezType(type_key) => {
                        index.faults.push(IndexFault {
                            set_name: set.name.clone(),
                            pieces: tier.pieces,
                            detail: format!("mez type {type_key:?}, which the calc does not route"),
                        });
                        continue;
                    }
                    NamedStats::Unknown => {
                        index.faults.push(IndexFault {
                            set_name: set.name.clone(),
                            pieces: tier.pieces,
                            detail: format!(
                                "{:?}, which is not in the stat vocabulary",
                                effect.stat
                            ),
                        });
                        continue;
                    }
                };

                // Two stats of one effect can name the same row, so collapse before counting:
                // that is what makes the expansion count below mean "lands on several DIFFERENT
                // rows".
                rows.dedup_by_key(|row| row.id);

                // A stat the vocabulary knows but no dashboard row carries. The registry gate
                // says this cannot happen; saying so out loud is what keeps it that way.
                if rows.is_empty() {
                    index.faults.push(IndexFault {
                        set_name: set.name.clone(),
                        pieces: tier.pieces,
                        detail: format!("{:?}, which no dashboard row carries", effect.stat),
                    });
                    continue;
                }

                // A Smashing/Lethal bonus lands on both rows and IS an expansion: under
                // "Smashing" alone the label would hide the Lethal half.
                let expanded = rows.len() > 1;
                for row in rows {
                    if !seen.insert((set_id.clone(), tier.pieces, row.id, effect.value.to_bits())) {
                        continue;
                    }
                    index.entries.push(LookupEntry {
                        set_name: set.name.clone(),
                        set_type: set.set_type.clone(),
                        category: set.category.clone(),
                        min_level: set.min_level,
                        max_level: set.max_level,
                        pieces: tier.pieces,
                        row,
                        value: effect.value,
                        pvp: effect.pvp,
                        description: format_bonus_desc(&effect.desc, &effect.stat, effect.value),
                        expanded,
                    });
                }
            }
        }
    }

    index
}

/// The set categories the build's own powers accept — the "sets I can slot" filter.
///
/// Read through each pick's def rather than off the selection, since `allowedSetCategories` is a
/// property of the power the export describes, not of having taken it. An empty result means the
/// build holds nothing sluttable yet, which the filter treats as "no restriction to apply" rather
/// than as "nothing qualifies" (the beta's `size === 0` guard) — a fresh build should not open on
/// an empty table.
fn slottable_categories(build: &CharacterState, database: &PowerDatabase) -> BTreeSet<String> {
    build
        .all_selected()
        .filter_map(|pick| database.resolve_power(&pick.powerset, &pick.internal_name))
        .filter_map(|power| power.allowed_set_categories.as_ref())
        .flatten()
        .cloned()
        .collect()
}

// ============================================================
// Sorting.
// ============================================================

/// A sortable results column. Set name is the initial sort (with pieces as its tiebreak), which
/// is the order the beta called "unsorted" — so its third, order-restoring click state has
/// nothing left to restore and the header is a plain two-state toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortColumn {
    SetName,
    Level,
    SetType,
    Effect,
    Value,
    Pieces,
}

impl SortColumn {
    /// The direction a column sorts on its first click. Numbers open descending because the
    /// question that brings someone to a numeric column is "which is biggest".
    fn opens_descending(self) -> bool {
        matches!(self, SortColumn::Value | SortColumn::Pieces)
    }

    fn label(self) -> &'static str {
        match self {
            SortColumn::SetName => "Set",
            SortColumn::Level => "Level",
            SortColumn::SetType => "Slots into",
            SortColumn::Effect => "Effect",
            SortColumn::Value => "Value",
            SortColumn::Pieces => "Pieces",
        }
    }

    const ALL: [SortColumn; 6] = [
        SortColumn::SetName,
        SortColumn::Level,
        SortColumn::SetType,
        SortColumn::Effect,
        SortColumn::Value,
        SortColumn::Pieces,
    ];
}

/// Order two results. Every column falls back to set name then pieces, so equal keys never
/// shuffle between renders.
fn compare(
    a: &LookupEntry,
    b: &LookupEntry,
    column: SortColumn,
    descending: bool,
) -> std::cmp::Ordering {
    let primary = match column {
        SortColumn::SetName => a.set_name.cmp(&b.set_name),
        SortColumn::Level => a.min_level.cmp(&b.min_level),
        SortColumn::SetType => a.set_type.cmp(&b.set_type),
        SortColumn::Effect => a.row.label.cmp(b.row.label),
        SortColumn::Value => a.value.total_cmp(&b.value),
        SortColumn::Pieces => a.pieces.cmp(&b.pieces),
    };
    let primary = if descending {
        primary.reverse()
    } else {
        primary
    };
    primary
        .then_with(|| a.set_name.cmp(&b.set_name))
        .then_with(|| a.pieces.cmp(&b.pieces))
}

// ============================================================
// The modal.
// ============================================================

/// The finder's open state, held at the shell root for the containment reason every modal here
/// shares: a `fixed` backdrop is contained by the grid's `transform`ed surfaces (see
/// [`crate::modal`]).
#[derive(Clone, Copy)]
pub struct SetBonusFinderOpen(pub Signal<bool>);

/// Mounted by the shell above both layout roots. Renders nothing while closed.
#[component]
pub fn SetBonusFinderHost(database: Option<Db>) -> Element {
    let mut open = use_context::<SetBonusFinderOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        FloatingPanel {
            id: "set-bonus-finder",
            title: "Set Bonus Finder".to_string(),
            // Wide: six columns, two of which carry game names long enough to be unrecognisable
            // truncated ("Ascendancy of the Dominator", "Dominator Archetype Set").
            default_size: (960.0, 720.0),
            on_close: move |_| open.set(false),
            SetBonusFinderBody { database }
        }
    }
}

/// The finder's two questions: which sets grant a bonus, and where in this build one could go.
#[derive(Clone, Copy, PartialEq)]
enum FinderView {
    BySet,
    ByPower,
}

#[component]
fn SetBonusFinderBody(database: Option<Db>) -> Element {
    let mut view = use_signal(|| FinderView::BySet);
    let option_class = move |option: FinderView| {
        if view() == option {
            "powers-layout-option is-active"
        } else {
            "powers-layout-option"
        }
    };
    rsx! {
        div { class: "bonus-finder-shell",
            div { class: "bonus-finder__views",
                div { class: "powers-layout", role: "radiogroup", "aria-label": "Finder view",
                    button {
                        class: option_class(FinderView::BySet),
                        role: "radio",
                        "aria-checked": view() == FinderView::BySet,
                        title: "Every set bonus in the dataset, searched by what it grants",
                        onclick: move |_| view.set(FinderView::BySet),
                        "By set"
                    }
                    button {
                        class: option_class(FinderView::ByPower),
                        role: "radio",
                        "aria-checked": view() == FinderView::ByPower,
                        title: "Where in this build a set with these bonuses could go",
                        onclick: move |_| view.set(FinderView::ByPower),
                        "By power"
                    }
                }
            }
            match view() {
                FinderView::BySet => rsx! { BySetView { database } },
                FinderView::ByPower => rsx! { ByPowerView { database } },
            }
        }
    }
}

#[component]
fn BySetView(database: Option<Db>) -> Element {
    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };
    let Some(catalog) = database.0.io_sets.as_ref() else {
        // A dataset with no IO-set section has no bonuses to find. Said plainly rather than
        // shown as an empty table, which would read as "your filters matched nothing".
        return rsx! {
            div { class: "load-state error", "This dataset ships no IO-set catalog." }
        };
    };

    let mut section = use_signal(|| None::<StatSection>);
    let mut row_id = use_signal(|| None::<&'static str>);
    let mut strength = use_signal(|| None::<u64>);
    let mut slottable_only = use_signal(|| false);
    let mut sort_column = use_signal(|| SortColumn::SetName);
    let mut descending = use_signal(|| false);

    // The index is a pure read of the catalog, so it is built once per dataset rather than per
    // keystroke: the filters below narrow this list, they never rebuild it.
    let index = use_memo({
        let catalog = catalog.clone();
        move || build_index(&catalog)
    });

    let session = use_context::<BuildSession>();
    let slottable = use_memo({
        let database = database.clone();
        move || slottable_categories(&session.build.read(), &database.0)
    });

    let picked_section = section();
    let picked_row = row_id();
    let picked_strength = strength();
    let restrict = slottable_only();

    // Everything the section filter admits — the pool the Type and Strength options are drawn
    // from, so an option can never be offered that yields nothing.
    let in_section: Vec<LookupEntry> = match picked_section {
        None => Vec::new(),
        Some(section) => index
            .read()
            .entries
            .iter()
            .filter(|entry| entry.row.section == section)
            .cloned()
            .collect(),
    };

    let types: Vec<&'static StatDef> = picked_section
        .map(|section| {
            stat_registry::in_section(section)
                .filter(|row| in_section.iter().any(|entry| entry.row.id == row.id))
                .collect()
        })
        .unwrap_or_default();

    let strengths: Vec<f64> = {
        let mut values: Vec<f64> = in_section
            .iter()
            .filter(|entry| picked_row.is_none_or(|id| entry.row.id == id))
            .map(|entry| entry.value)
            .collect();
        values.sort_by(|a, b| a.total_cmp(b));
        values.dedup_by(|a, b| a.to_bits() == b.to_bits());
        values
    };

    let mut results: Vec<LookupEntry> = in_section
        .iter()
        .filter(|entry| picked_row.is_none_or(|id| entry.row.id == id))
        .filter(|entry| picked_strength.is_none_or(|bits| entry.value.to_bits() == bits))
        .filter(|entry| !restrict || can_slot(entry, &slottable.read()))
        .cloned()
        .collect();
    results.sort_by(|a, b| compare(a, b, sort_column(), descending()));

    let faults = index.read().faults.clone();

    rsx! {
        div { class: "bonus-finder",
            div { class: "bonus-finder__filters",
                label { class: "bonus-finder__filter",
                    span { class: "field-label", "Effect" }
                    select {
                        class: "build-select",
                        onchange: move |evt| {
                            section.set(section_by_title(&evt.value()));
                            row_id.set(None);
                            strength.set(None);
                        },
                        option { value: "", selected: picked_section.is_none(), "Select an effect…" }
                        for candidate in StatSection::ALL {
                            option {
                                key: "{candidate:?}",
                                value: "{candidate.title()}",
                                selected: picked_section == Some(candidate),
                                "{candidate.title()}"
                            }
                        }
                    }
                }
                label { class: "bonus-finder__filter",
                    span { class: "field-label", "Type" }
                    select {
                        class: "build-select",
                        disabled: types.is_empty(),
                        onchange: move |evt| {
                            row_id.set(stat_registry::by_id(&evt.value()).map(|row| row.id));
                            strength.set(None);
                        },
                        option { value: "", selected: picked_row.is_none(), "Any" }
                        for row in types.iter() {
                            option {
                                key: "{row.id}",
                                value: "{row.id}",
                                selected: picked_row == Some(row.id),
                                "{row.label}"
                            }
                        }
                    }
                }
                label { class: "bonus-finder__filter",
                    span { class: "field-label", "Strength" }
                    select {
                        class: "build-select",
                        disabled: strengths.is_empty(),
                        onchange: move |evt| {
                            strength.set(evt.value().parse::<f64>().ok().map(f64::to_bits));
                        },
                        option { value: "", selected: picked_strength.is_none(), "Any" }
                        for value in strengths.iter().copied() {
                            option {
                                key: "{value.to_bits()}",
                                value: "{value}",
                                selected: picked_strength == Some(value.to_bits()),
                                "{format_value(value)}"
                            }
                        }
                    }
                }
                label { class: "bonus-finder__restrict",
                    input {
                        r#type: "checkbox",
                        checked: restrict,
                        onchange: move |evt| slottable_only.set(evt.checked()),
                    }
                    "Only sets my powers can take"
                }
            }

            if !faults.is_empty() {
                // Above the results and folded. Above, because a warning under a two-thousand-row
                // table is a warning nobody reads; folded, because these are a property of the
                // CATALOG rather than of the search — the same rows are missing from every query
                // on this dataset, so the count belongs permanently in view while the identical
                // sentences behind it do not.
                details { class: "bonus-finder__faults",
                    summary { class: "bonus-finder__faults-title",
                        "{faults.len()} bonus{plural_es(faults.len())} in this dataset could not be filed, and are missing from the results"
                    }
                    for fault in faults.iter() {
                        div { class: "bonus-finder__fault",
                            key: "{fault.set_name}-{fault.pieces}-{fault.detail}",
                            "{fault.set_name} {fault.pieces}pc — {fault.detail}"
                        }
                    }
                }
            }

            if picked_section.is_none() {
                div { class: "bonus-finder__empty",
                    "Pick an effect to search every set bonus in the dataset."
                }
            } else if results.is_empty() {
                div { class: "bonus-finder__empty", "No set bonus matches those filters." }
            } else {
                div { class: "bonus-finder__table", role: "table",
                    div { class: "bonus-finder__head", role: "row",
                        for column in SortColumn::ALL {
                            SortHeader {
                                key: "{column:?}",
                                column,
                                active: sort_column() == column,
                                descending: descending(),
                                on_sort: move |picked: SortColumn| {
                                    if sort_column() == picked {
                                        descending.toggle();
                                    } else {
                                        sort_column.set(picked);
                                        descending.set(picked.opens_descending());
                                    }
                                },
                            }
                        }
                    }
                    for entry in results.iter() {
                        ResultRow {
                            key: "{entry.set_name}-{entry.pieces}-{entry.row.id}-{entry.value}",
                            entry: entry.clone(),
                            dimmed: !restrict && !can_slot(entry, &slottable.read()),
                        }
                    }
                }
                div { class: "bonus-finder__footer",
                    span { "{results.len()} result{plural(results.len())}" }
                    span { class: "bonus-finder__legend", "⚔ applies in PvP zones only" }
                }
            }
        }
    }
}

/// The By power view: pick the bonuses being chased, and see each power in the build that could
/// carry a set granting them, with what it would cost ([`crate::bonus_by_power`]).
#[component]
fn ByPowerView(database: Option<Db>) -> Element {
    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };
    let Some(catalog) = database.0.io_sets.as_ref() else {
        return rsx! {
            div { class: "load-state error", "This dataset ships no IO-set catalog." }
        };
    };
    let session = use_context::<BuildSession>();
    let totals = use_context::<crate::panels::stats::BuildTotals>().0;
    let tracked = use_context::<crate::tracked_stats::TrackedStats>();
    let mut chosen = use_signal(Vec::<String>::new);
    let mut every_set = use_signal(|| false);

    // The add menu counts the sets this build's powers can take, so a number reads as "places
    // to look", not as the whole catalog.
    let offered = {
        let slottable = slottable_categories(&session.build.read(), &database.0);
        let sets: Vec<(&str, &coh_data::IoSet)> = catalog
            .sets
            .iter()
            .filter(|(_, set)| slottable.is_empty() || slottable.contains(&set.set_type))
            .map(|(id, set)| (id.as_str(), set))
            .collect();
        crate::picker_sets::bonus_counts(&sets, &crate::picker_sets::SetFacets::default())
            .into_iter()
            .filter(|count| !chosen.read().iter().any(|id| id == count.stat.id))
            .collect::<Vec<_>>()
    };
    let chosen_now = chosen();
    let tracking = totals.read().set_bonus_tracking.clone();
    let found = crate::bonus_by_power::routes(
        &session.build.read(),
        &database.0,
        catalog,
        &chosen_now,
        &tracking,
    );
    let places = found.len();
    let shown: Vec<(crate::bonus_by_power::Route, usize)> = if every_set() {
        found.into_iter().map(|route| (route, 0)).collect()
    } else {
        crate::bonus_by_power::best_per_power(found)
    };
    let tracked_ids = tracked.0.read().clone();
    let tracked_new = tracked_ids.iter().any(|id| !chosen_now.contains(id));

    rsx! {
        div { class: "bonus-finder",
            div { class: "bonus-routes__choose",
                crate::panels::powers::BonusSelect {
                    counts: offered,
                    selected: None,
                    placeholder: "Add a bonus…",
                    label: "Add a bonus to look for",
                    onchange: move |picked: Option<String>| {
                        if let Some(id) = picked {
                            chosen.with_mut(|list| {
                                if !list.contains(&id) {
                                    list.push(id);
                                }
                            });
                        }
                    },
                }
                button {
                    class: "picker-chip",
                    r#type: "button",
                    disabled: !tracked_new,
                    title: if tracked_ids.is_empty() {
                        "Track a stat in a stats panel first"
                    } else {
                        "Look for every stat you track"
                    },
                    onclick: move |_| {
                        let ids = tracked.0.read().clone();
                        chosen.with_mut(|list| {
                            for id in ids {
                                if !list.contains(&id) {
                                    list.push(id);
                                }
                            }
                        });
                    },
                    "Use tracked stats"
                }
                label { class: "bonus-finder__restrict",
                    input {
                        r#type: "checkbox",
                        checked: every_set(),
                        onchange: move |evt| every_set.set(evt.checked()),
                    }
                    "Every set, not just the best one per power"
                }
            }
            if !chosen_now.is_empty() {
                // What the build already holds of each chosen bonus, value by value: the Rule of
                // 5 counts each value separately, so a stat can be full at +3% and open at +1.5%.
                div { class: "bonus-routes__held",
                    for id in chosen_now.iter() {
                        if let Some(row) = stat_registry::by_id(id) {
                            div { class: "bonus-routes__held-row", key: "{id}",
                                button {
                                    class: "picker-chip is-on",
                                    r#type: "button",
                                    title: "Stop looking for this bonus",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| chosen.with_mut(|list| list.retain(|kept| kept != &id))
                                    },
                                    "{row.label} {row.section.title().to_lowercase()} ✕"
                                }
                                span { class: "bonus-routes__copies",
                                    {held_summary(row, &tracking)}
                                }
                            }
                        }
                    }
                }
            }
            if chosen_now.is_empty() {
                div { class: "bonus-finder__empty",
                    "Choose a bonus, or use the stats you track, to see where in this build a set granting it could go."
                }
            } else if shown.is_empty() {
                div { class: "bonus-finder__empty",
                    "No power in this build can take a set with those bonuses."
                }
            } else {
                div { class: "bonus-routes",
                    for (route , others) in shown.iter() {
                        RouteRow {
                            key: "{route.power_name}-{route.power_level}-{route.set_id}",
                            route: route.clone(),
                            others: *others,
                        }
                    }
                }
                div { class: "bonus-finder__footer",
                    span {
                        if every_set() {
                            "{places} place{plural(places)}"
                        } else {
                            "{shown.len()} power{plural(shown.len())}, {places} place{plural(places)} in all"
                        }
                    }
                }
            }
        }
    }
}

/// "+3% × 4 of 5 · +1.5% × 5 of 5, full" for one chosen bonus, or a plain "none yet".
fn held_summary(row: &StatDef, tracking: &[set_bonuses::SetBonusStatTracking]) -> String {
    let copies = crate::bonus_by_power::held_copies(row, tracking);
    if copies.is_empty() {
        return "You hold none yet".to_string();
    }
    let parts: Vec<String> = copies
        .iter()
        .map(|(value, held, capped)| {
            let full = if *capped || *held >= 5 { ", full" } else { "" };
            format!("+{} × {} of 5{full}", format_value(*value), held.min(&5))
        })
        .collect();
    format!("You hold {}", parts.join(" · "))
}

/// One place a set could go: the power, the set, what it grants, and what it would cost.
#[component]
fn RouteRow(route: crate::bonus_by_power::Route, others: usize) -> Element {
    use crate::bonus_by_power::Cost;
    let spent = route.all_spent();
    let cost = match &route.cost {
        Cost::Slotted => "Already slotted here".to_string(),
        Cost::Fits => "Fits in its slots".to_string(),
        Cost::Replaces(names) => format!("Replaces {}", names.join(", ")),
        Cost::NeedsSlots { more, replaces } if replaces.is_empty() => {
            format!("Needs {more} more slot{}", plural(*more))
        }
        Cost::NeedsSlots { more, replaces } => format!(
            "Needs {more} more slot{} and replaces {}",
            plural(*more),
            replaces.join(", ")
        ),
    };
    let cost_class = match route.cost {
        Cost::Slotted | Cost::Fits => "bonus-route__cost is-free",
        _ => "bonus-route__cost",
    };
    rsx! {
        div {
            class: if spent { "bonus-route is-spent" } else { "bonus-route" },
            title: if spent { "Every bonus here would be past the Rule of 5 cap and grant nothing" } else { "" },
            span { class: "bonus-route__power",
                span { class: "bonus-route__power-name", "{route.power_name}" }
                span { class: "bonus-route__power-meta",
                    if route.power_level > 0 {
                        "L{route.power_level} · "
                    }
                    "{route.slots} slot{plural(route.slots)}"
                }
            }
            span { class: "bonus-finder__set",
                span { class: "bonus-finder__rarity", "{route.set_category}" }
                span { class: "bonus-finder__set-name", "{route.set_name}" }
                if others > 0 {
                    span { class: "bonus-route__others", "+{others} other set{plural(others)}" }
                }
            }
            span { class: "bonus-route__grants",
                for grant in route.grants.iter() {
                    span {
                        key: "{grant.row.id}",
                        class: if grant.spent { "picker-tracked-chip is-capped" } else { "picker-tracked-chip" },
                        title: if grant.spent { "Already five copies of this bonus; another grants nothing" } else { "" },
                        "{grant.pieces}pc {grant.row.label} +{format_value(grant.value)} ({grant.held}/5)"
                    }
                }
            }
            span { class: "{cost_class}",
                "{cost}"
                if spent {
                    span { class: "bonus-route__spent", " · past the Rule of 5" }
                }
            }
        }
    }
}

/// One sortable column header. Carries the direction only while it is the active column — an
/// arrow on every header would claim six sorts are in effect.
#[component]
fn SortHeader(
    column: SortColumn,
    active: bool,
    descending: bool,
    on_sort: EventHandler<SortColumn>,
) -> Element {
    let arrow = match (active, descending) {
        (false, _) => "",
        (true, false) => " ↑",
        (true, true) => " ↓",
    };
    rsx! {
        button {
            class: if active { "bonus-finder__col is-sorted" } else { "bonus-finder__col" },
            r#type: "button",
            "aria-sort": if active {
                Some(if descending { "descending" } else { "ascending" })
            } else {
                None
            },
            onclick: move |_| on_sort.call(column),
            "{column.label()}{arrow}"
        }
    }
}

#[component]
fn ResultRow(entry: LookupEntry, dimmed: bool) -> Element {
    let level = if entry.min_level == entry.max_level {
        format!("{}", entry.min_level)
    } else {
        format!("{}–{}", entry.min_level, entry.max_level)
    };
    rsx! {
        div {
            class: if dimmed { "bonus-finder__row is-unslottable" } else { "bonus-finder__row" },
            role: "row",
            span { class: "bonus-finder__set",
                span { class: "bonus-finder__rarity", "{entry.category}" }
                span { class: "bonus-finder__set-name", "{entry.set_name}" }
            }
            span { class: "bonus-finder__level", "{level}" }
            span { class: "bonus-finder__type", "{entry.set_type}" }
            span { class: "bonus-finder__effect",
                span { class: "bonus-finder__effect-row", "{entry.row.label}" }
                if entry.expanded {
                    span { class: "bonus-finder__effect-desc", "{entry.description}" }
                }
            }
            span { class: "bonus-finder__value",
                "{format_value(entry.value)}"
                if entry.pvp {
                    span { class: "bonus-finder__pvp", title: "PvP zones only", " ⚔" }
                }
            }
            span { class: "bonus-finder__pieces", "{entry.pieces}" }
        }
    }
}

/// Whether the build holds a power that can take this set. An empty category set means the build
/// has nothing slottable yet, and every set reads as available rather than none.
fn can_slot(entry: &LookupEntry, slottable: &BTreeSet<String>) -> bool {
    slottable.is_empty() || slottable.contains(&entry.set_type)
}

/// Resolve a section from its own title — the value the `select` round-trips. Titles come from
/// the panel each section renders ([`StatSection::title`]), so this cannot drift from the list
/// the options were built from.
fn section_by_title(title: &str) -> Option<StatSection> {
    StatSection::ALL
        .into_iter()
        .find(|section| section.title() == title)
}

/// A bonus value as a percentage, trailing zeros trimmed. Three decimals, the precision the
/// export authors and the same one [`format_bonus_desc`] splices, so a row's number and its
/// description can never disagree — an ATO's `+1.525% Damage` is not `1.52%`.
fn format_value(value: f64) -> String {
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed}%")
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn plural_es(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "es"
    }
}
