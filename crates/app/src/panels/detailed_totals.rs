//! The detailed-totals stat sheet (the beta `DetailedTotalsModal`): every dashboard stat with
//! its sources, grouped by where each contribution came from.
//!
//! The dashboard answers *what is my defense*; this answers *why*. It reads the engine's
//! per-source provenance ledgers — no second calculation, and nothing here decides a number.
//!
//! # The rows must add up, and say so when they don't
//!
//! Every group below comes from a ledger the engine measured by diffing its own accumulator
//! around each contributor, and `coh_math`'s `ledger_reconciliation_corpus` holds every
//! attributable field to the rule that its total equals the sum of its live rows. That gate is
//! what makes this surface trustworthy — but it grades the CORPUS build, not the one in front of
//! the user, and a pass added later could write a field down a path no ledger brackets. So each
//! key still shows an [`KeyBreakdown::residual`] whenever its rows fall short, rather than
//! letting the difference go unmentioned: a stat whose sources quietly sum to less than the
//! number above them is the one failure mode this whole design exists to prevent, and it is
//! invisible without a line that names it.
//!
//! # Rows that are shown but did not apply
//!
//! Three kinds of contribution are real, visible, and NOT in the total: a set bonus the Rule of 5
//! rejected, a travel buff that lost its suppress group, and a stealth radius superseded by a
//! larger one. Each keeps its row, marked, because a build paying for a bonus it does not receive
//! should be told so — the same reason the engine keeps them in its ledgers instead of dropping
//! them. Only [`RowState::Applied`] rows are summed.
//!
//! # Why the breakdown is keyed by LEDGER key, not by stat
//!
//! A dashboard stat is not always one accumulator field. `Smashing/Lethal` defense is the MAX of
//! two, Max HP shows absolute hit points projected from a percentage, and a travel row shows mph
//! projected from a buff percent. So each stat expands to one section per
//! [`StatDef::ledger_keys`](crate::panels::stat_registry::StatDef::ledger_keys), each with its
//! own total in its own unit — rather than one flat list under a face value the rows could not
//! sum to. That also keeps the reconciliation meaningful: it is a per-key claim, and this
//! renders it per key.

use crate::floating::FloatingPanel;
use crate::naming::power_label;
use crate::panels::stat_registry::{self, StatDef, StatSection};
use crate::panels::stats::BuildTotals;
use crate::shell::Db;
use coh_math::CalculatedTotals;
use dioxus::prelude::*;

/// Where a contribution came from — the beta's six `SOURCE_GROUPS`, in the order it lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceGroup {
    SetBonus,
    ActivePower,
    Inherent,
    Accolade,
    Proc,
    Incarnate,
}

impl SourceGroup {
    pub fn label(self) -> &'static str {
        match self {
            SourceGroup::SetBonus => "Set Bonuses",
            SourceGroup::ActivePower => "Active Powers",
            SourceGroup::Inherent => "Inherent Powers",
            SourceGroup::Accolade => "Accolades",
            SourceGroup::Proc => "Procs",
            SourceGroup::Incarnate => "Incarnate Powers",
        }
    }

    /// The order groups are listed in, largest-and-most-actionable first. Its own list rather
    /// than the enum's declaration order so the two can be reasoned about separately.
    pub const ORDER: [SourceGroup; 6] = [
        SourceGroup::SetBonus,
        SourceGroup::ActivePower,
        SourceGroup::Inherent,
        SourceGroup::Accolade,
        SourceGroup::Proc,
        SourceGroup::Incarnate,
    ];

    fn slug(self) -> &'static str {
        match self {
            SourceGroup::SetBonus => "setbonus",
            SourceGroup::ActivePower => "activepower",
            SourceGroup::Inherent => "inherent",
            SourceGroup::Accolade => "accolade",
            SourceGroup::Proc => "proc",
            SourceGroup::Incarnate => "incarnate",
        }
    }
}

/// Whether a row reached the total, and if not, why not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    /// Counted in the total.
    Applied,
    /// A set bonus past the Rule of 5's fifth identical copy. The build holds it; it grants
    /// nothing.
    Rejected,
    /// Lost its suppress group — a travel buff beaten by a stronger one, or a stealth radius
    /// superseded. Ordinary game mechanics, NOT a cap.
    Superseded,
}

impl RowState {
    pub fn note(self) -> &'static str {
        match self {
            RowState::Applied => "",
            RowState::Rejected => "Rule of 5: this copy won't provide a bonus",
            RowState::Superseded => "Superseded; a stronger source in the same group applies",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            RowState::Applied => "applied",
            RowState::Rejected => "rejected",
            RowState::Superseded => "superseded",
        }
    }
}

/// One contributor.
#[derive(Clone, PartialEq, Debug)]
pub struct SourceRow {
    /// What granted it, as a player would name it.
    pub label: String,
    /// The qualifier under the name — an IO set's tier, an incarnate slot. Empty when the label
    /// says everything.
    pub detail: String,
    pub value: f64,
    pub state: RowState,
}

/// One accumulator field's worth of sources, with the total they must explain.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyBreakdown {
    pub key: &'static str,
    /// The engine's own value for this field — never a sum of the rows below.
    pub total: f64,
    pub groups: Vec<(SourceGroup, Vec<SourceRow>)>,
    /// `total` minus the applied rows. Zero on every build the reconciliation gate covers; a
    /// non-zero value means a contribution reached the total with nothing accounting for it, and
    /// the modal says so in as many words rather than showing a short list under a bigger number.
    pub residual: f64,
}

impl KeyBreakdown {
    pub fn has_rows(&self) -> bool {
        self.groups.iter().any(|(_, rows)| !rows.is_empty())
    }

    /// A residual worth drawing. The floor is the display tier's own precision — a difference
    /// that rounds away at two decimals is float noise from re-adding the rows in a different
    /// order than the passes did, not a missing contributor.
    pub fn unexplained(&self) -> Option<f64> {
        (self.residual.abs() >= 0.005).then_some(self.residual)
    }
}

/// Every source feeding one accumulator field, grouped and ordered for display.
pub fn breakdown_for_key(
    totals: &CalculatedTotals,
    db: Option<&Db>,
    key: &'static str,
) -> KeyBreakdown {
    use coh_math::gather::PowerSourceKind;

    let mut grouped: Vec<(SourceGroup, Vec<SourceRow>)> = SourceGroup::ORDER
        .iter()
        .map(|g| (*g, Vec::new()))
        .collect();
    let mut push = |group: SourceGroup, row: SourceRow| {
        if let Some((_, rows)) = grouped.iter_mut().find(|(g, _)| *g == group) {
            rows.push(row);
        }
    };

    // Set bonuses — through the engine's own shared derivation, which owns the two special
    // routings (the KB ×0.01 scale, the Recharge-debuff fan-out to Slow). Re-deriving them here
    // would be a second copy of the routing free to drift from the totals.
    for source in coh_math::set_bonuses::breakdown_sources_for_key(&totals.set_bonus_tracking, key)
    {
        push(
            SourceGroup::SetBonus,
            SourceRow {
                label: source.set_name.clone(),
                detail: format!(
                    "{}pc · {}",
                    source.pieces,
                    power_label(db, &source.power_set, &source.power_internal_name)
                ),
                value: source.value,
                state: if source.rejected {
                    RowState::Rejected
                } else {
                    RowState::Applied
                },
            },
        );
    }

    // Powers, accolades and archetype inherents — one ledger, and the ROW's `kind` names the
    // group rather than the field it arrived in (an accolade and a toggle are both auto-on Self
    // powers by the time the apply pass sees them).
    for row in totals
        .power_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        let group = match row.kind {
            PowerSourceKind::ActivePower => SourceGroup::ActivePower,
            PowerSourceKind::Accolade => SourceGroup::Accolade,
            PowerSourceKind::Inherent => SourceGroup::Inherent,
        };
        push(
            group,
            SourceRow {
                label: power_label(db, &row.power_set, &row.power_internal_name),
                detail: String::new(),
                value: row.value,
                state: RowState::Applied,
            },
        );
    }

    // Travel and stealth are powers too — they carry their own ledgers only because both totals
    // are grouped-max resolves committed after the walk, not because they are a different kind
    // of source. Both already hold a display name.
    for row in totals
        .movement_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        push(
            SourceGroup::ActivePower,
            SourceRow {
                label: row.power_name.clone(),
                detail: String::new(),
                value: row.value,
                state: if row.suppressed {
                    RowState::Superseded
                } else {
                    RowState::Applied
                },
            },
        );
    }
    for row in totals
        .stealth_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        push(
            SourceGroup::ActivePower,
            SourceRow {
                label: row.power_name.clone(),
                detail: String::new(),
                value: row.value,
                state: if row.superseded {
                    RowState::Superseded
                } else {
                    RowState::Applied
                },
            },
        );
    }
    for row in totals
        .buff_pet_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        push(
            SourceGroup::ActivePower,
            SourceRow {
                label: power_label(db, &row.power_set, &row.power_internal_name),
                detail: "pet aura".to_string(),
                value: row.value,
                state: RowState::Applied,
            },
        );
    }

    for row in totals
        .proc_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        push(
            SourceGroup::Proc,
            SourceRow {
                label: row.proc_name.clone(),
                detail: format!(
                    "{} · {}",
                    row.set_name,
                    power_label(db, &row.power_set, &row.power_internal_name)
                ),
                value: row.value,
                // A capped proc is the Rule of 5 rejecting it, the same fact the set-bonus rows
                // carry under the same name.
                state: if row.capped {
                    RowState::Rejected
                } else {
                    RowState::Applied
                },
            },
        );
    }

    for row in totals
        .incarnate_breakdown
        .iter()
        .filter(|r| r.breakdown_key == key)
    {
        push(
            SourceGroup::Incarnate,
            SourceRow {
                label: row.power_name.clone(),
                detail: if row.exemplar {
                    format!("{} · exemplar", row.slot)
                } else {
                    row.slot.clone()
                },
                value: row.value,
                state: RowState::Applied,
            },
        );
    }

    // Biggest contributor first inside each group — the question a breakdown answers is "what is
    // carrying this number", and that is the order that answers it.
    for (_, rows) in grouped.iter_mut() {
        rows.sort_by(|a, b| {
            b.value
                .abs()
                .partial_cmp(&a.value.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let total = totals.bonuses.get(key).unwrap_or(0.0);
    let applied: f64 = grouped
        .iter()
        .flat_map(|(_, rows)| rows)
        .filter(|row| row.state == RowState::Applied)
        .map(|row| row.value)
        .sum();

    KeyBreakdown {
        key,
        total,
        groups: grouped,
        residual: total - applied,
    }
}

/// Every key breakdown behind one dashboard stat, in the order the stat names them.
///
/// Empty for a stat with no ledger keys — a derived rate or a baseline, which has no per-source
/// explanation to give ([`GlobalBonuses::is_attributable`]). The caller offers no expansion
/// there rather than an expansion that opens onto nothing.
pub fn breakdowns_for_stat(
    totals: &CalculatedTotals,
    db: Option<&Db>,
    stat: &StatDef,
) -> Vec<KeyBreakdown> {
    stat.ledger_keys
        .iter()
        .map(|key| breakdown_for_key(totals, db, key))
        .collect()
}

/// Whether a stat can be expanded at all — it names at least one attributable key AND some
/// ledger actually has something to say about it on this build.
pub fn is_expandable(breakdowns: &[KeyBreakdown]) -> bool {
    breakdowns
        .iter()
        .any(|breakdown| breakdown.has_rows() || breakdown.unexplained().is_some())
}

// ============================================================
// The floating panel.
// ============================================================

/// The sheet's open state, held at the shell root for the containment reason every modal here
/// shares: each grid surface is its own stacking context, so anything rendered inside one is
/// trapped beneath its neighbours (see [`crate::modal`]).
#[derive(Clone, Copy)]
pub struct DetailedTotalsOpen(pub Signal<bool>);

/// The sheet, mounted by the shell above both layout roots. Renders nothing while closed.
///
/// A floating panel rather than a modal: this is Mids' "View Totals" window, which players keep
/// open beside the build and watch while they slot. A modal would have to be closed to make
/// the change it is explaining. The body reads the shared build memo, so it follows every edit.
#[component]
pub fn DetailedTotalsHost(database: Option<Db>) -> Element {
    let mut open = use_context::<DetailedTotalsOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        FloatingPanel {
            id: "detailed-totals",
            title: "Detailed Totals".to_string(),
            // Wide enough for the sections to sit two abreast, as the modal had them.
            default_size: (960.0, 720.0),
            on_close: move |_| open.set(false),
            DetailedTotalsBody { database }
        }
    }
}

#[component]
fn DetailedTotalsBody(database: Option<Db>) -> Element {
    let totals = use_context::<BuildTotals>().0;

    // One read of the shared build memo for the whole sheet — the dashboard's compute-once
    // contract. Every row below is a projection of this value; nothing here recalculates.
    let totals = totals.read();

    rsx! {
        div { class: "detailed-totals",
            div { class: "detailed-totals-summary",
                "Every stat the planner tracks, with the sources behind it. "
                "Bars fill toward the stat's cap where it has one, and otherwise compare it with "
                "the largest like-for-like stat in its section. "
                "Sources that are shown but struck through were not applied."
            }
            div { class: "detailed-totals-grid",
                for section in StatSection::ALL {
                    DetailedTotalsSection {
                        key: "{section:?}",
                        section,
                        totals: totals.clone(),
                        db: database.clone(),
                    }
                }
            }
        }
    }
}

/// How full a stat's bar is drawn, and what it is measured against.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StatBar {
    /// `0.0..=1.0` of the bar's width.
    pub fill: f64,
    pub negative: bool,
    /// Measured against the stat's own ceiling rather than its neighbours. Drawn with an end
    /// mark, because "full" then means "at the cap" — a claim a relative bar never makes.
    pub against_cap: bool,
}

/// The bar for one stat. A capped stat fills toward its ceiling. An uncapped one is drawn
/// against the largest stat in its section that shares its unit — a bar needs SOMETHING to be
/// a fraction of, and a +10% beside a +24% is a comparison a reader can use, where a bar
/// against an invented maximum would be a claim about a number the game never names.
///
/// `None` when there is nothing honest to compare against: a stat alone in its unit (Recharge,
/// the endurance rates) would always draw full, which says nothing.
pub fn stat_bar(
    stat: &StatDef,
    value: f64,
    ceiling: Option<f64>,
    peers: &[(&'static StatDef, f64)],
) -> Option<StatBar> {
    let negative = value < 0.0;
    if let Some(ceiling) = ceiling {
        return Some(StatBar {
            fill: (value.abs() / ceiling).min(1.0),
            negative,
            against_cap: true,
        });
    }
    let same_unit = peers
        .iter()
        .filter(|(peer, _)| peer.format == stat.format && peer.cap == stat.cap);
    if same_unit.clone().count() < 2 {
        return None;
    }
    let peak = same_unit.map(|(_, v)| v.abs()).fold(0.0, f64::max);
    Some(StatBar {
        fill: if peak > 0.0 { value.abs() / peak } else { 0.0 },
        negative,
        against_cap: false,
    })
}

/// A stat that is zero and has no source to show — folded into one line per section so the
/// sheet spends its rows on the stats this build actually moves. Still listed by name: the
/// sheet is the full accounting, and "this is zero" is an answer someone came looking for.
fn is_idle(value: f64, breakdowns: &[KeyBreakdown]) -> bool {
    value.abs() < 1e-9 && !is_expandable(breakdowns)
}

/// One dashboard section's stats. The sheet shows EVERY stat in the vocabulary, not only the ones
/// the dashboard is configured to display: the dashboard is a chosen summary and this is the full
/// accounting, so hiding a row here would fail the user precisely when they came looking it up.
#[component]
fn DetailedTotalsSection(
    section: StatSection,
    totals: CalculatedTotals,
    db: Option<Db>,
) -> Element {
    let stats: Vec<(&'static StatDef, f64)> = stat_registry::in_section(section)
        .map(|stat| (stat, (stat.read)(&totals)))
        .collect();

    let mut active = Vec::new();
    let mut idle: Vec<&'static str> = Vec::new();
    for (stat, value) in &stats {
        let breakdowns = breakdowns_for_stat(&totals, db.as_ref(), stat);
        if is_idle(*value, &breakdowns) {
            idle.push(stat.label);
        } else {
            let bar = stat_bar(stat, *value, stat.cap.ceiling(&totals), &stats);
            active.push((*stat, breakdowns, bar));
        }
    }

    rsx! {
        section { class: "detailed-totals-section",
            h3 { class: "detailed-totals-section-title", "{section.title()}" }
            div { class: "detailed-totals-rows",
                for (stat, breakdowns, bar) in active {
                    DetailedStatRow {
                        key: "{stat.id}",
                        stat_id: stat.id,
                        totals: totals.clone(),
                        breakdowns,
                        bar,
                    }
                }
            }
            if !idle.is_empty() {
                div { class: "detailed-totals-idle",
                    span { class: "detailed-totals-idle-label", "At zero" }
                    "{idle.join(\" · \")}"
                }
            }
        }
    }
}

/// One stat: its face value and bar, and — when some ledger has something to say — an
/// expandable accounting of where that value came from.
#[component]
fn DetailedStatRow(
    stat_id: String,
    totals: CalculatedTotals,
    breakdowns: Vec<KeyBreakdown>,
    bar: Option<StatBar>,
) -> Element {
    let mut expanded = use_signal(|| false);

    let Some(stat) = stat_registry::by_id(&stat_id) else {
        // A row naming a stat the registry does not carry is a bug, not a blank line (Rule 1).
        return rsx! {
            div { class: "detailed-stat-row is-faulted", "Unknown stat {stat_id}" }
        };
    };

    let resolved = stat.resolve(&totals);
    let expandable = is_expandable(&breakdowns);
    let is_open = expanded();
    let bar_title = bar.map(|bar| {
        if bar.against_cap {
            format!("{:.0}% of the cap", bar.fill * 100.0)
        } else {
            "Relative to the largest like stat in this section".to_string()
        }
    });

    rsx! {
        div { class: "detailed-stat", style: "--stat-hue: {stat.family.token()}",
            button {
                class: if expandable { "detailed-stat-row is-expandable" } else { "detailed-stat-row" },
                r#type: "button",
                disabled: !expandable,
                "aria-expanded": if expandable { Some(is_open.to_string()) } else { None },
                onclick: move |_| expanded.toggle(),
                span {
                    class: if is_open {
                        "detailed-stat-caret"
                    } else {
                        "detailed-stat-caret caret--folded"
                    },
                    // A stat with no accounting to open draws no caret at all — the row is
                    // `disabled`, and a mark that never turns would say otherwise.
                    if expandable { "▼" }
                }
                span { class: "detailed-stat-label", title: "{stat.label}", "{stat.label}" }
                span { class: "detailed-stat-bar", title: bar_title,
                    if let Some(bar) = bar {
                        span {
                            class: "detailed-meter",
                            class: if bar.against_cap { "is-capped" },
                            class: if bar.negative { "is-negative" },
                            span { class: "detailed-meter-fill", style: "width: {bar.fill * 100.0:.1}%" }
                        }
                    }
                }
                span {
                    class: if resolved.at_cap { "detailed-stat-value is-at-cap" } else { "detailed-stat-value" },
                    title: if resolved.note.is_empty() { None } else { Some(resolved.note.clone()) },
                    "{resolved.text}"
                }
            }
            if is_open {
                div { class: "detailed-stat-body",
                    for breakdown in breakdowns.clone() {
                        KeyBreakdownBlock {
                            key: "{breakdown.key}",
                            breakdown: breakdown.clone(),
                            ledger_format: stat.ledger_format,
                            // A single-key stat needs no per-key heading: its total IS the row
                            // above. A multi-key one does, because the face is a projection
                            // (a max, a percentage turned into mph) that no single key equals.
                            show_key_heading: stat.ledger_keys.len() > 1,
                        }
                    }
                }
            }
        }
    }
}

/// How much each source group contributes, for the stacked bar over a key's sources. `None`
/// unless every applied row is a gain: a stack of shares only means something when the parts
/// add up to the whole, and a debuff among them would draw a segment bigger than the total.
fn group_shares(breakdown: &KeyBreakdown) -> Option<Vec<(SourceGroup, f64)>> {
    let applied = || {
        breakdown
            .groups
            .iter()
            .flat_map(|(_, rows)| rows)
            .filter(|row| row.state == RowState::Applied)
    };
    if applied().any(|row| row.value < 0.0) {
        return None;
    }
    let whole: f64 = applied().map(|row| row.value).sum();
    if whole <= 0.0 {
        return None;
    }
    let shares: Vec<(SourceGroup, f64)> = breakdown
        .groups
        .iter()
        .map(|(group, rows)| {
            let sum: f64 = rows
                .iter()
                .filter(|row| row.state == RowState::Applied)
                .map(|row| row.value)
                .sum();
            (*group, sum / whole)
        })
        .filter(|(_, share)| *share > 0.0)
        .collect();
    // One group is the whole bar, which tells the reader nothing the group title doesn't.
    (shares.len() > 1).then_some(shares)
}

/// One accumulator field's sources, under its own total.
#[component]
fn KeyBreakdownBlock(
    breakdown: KeyBreakdown,
    ledger_format: stat_registry::StatFormat,
    show_key_heading: bool,
) -> Element {
    // Every source bar is drawn against the biggest one under this key, so the longest bar is
    // the source carrying the most of it.
    let peak = breakdown
        .groups
        .iter()
        .flat_map(|(_, rows)| rows)
        .map(|row| row.value.abs())
        .fold(0.0, f64::max);
    let width = |value: f64| {
        if peak > 0.0 {
            value.abs() / peak * 100.0
        } else {
            0.0
        }
    };
    let shares = group_shares(&breakdown);

    rsx! {
        div { class: "detailed-key",
            if show_key_heading {
                div { class: "detailed-key-heading",
                    span { "{breakdown.key}" }
                    span { class: "mono", "{ledger_format.render(breakdown.total)}" }
                }
            }
            if let Some(shares) = shares {
                div { class: "detailed-share", "aria-hidden": "true",
                    for (group, share) in shares {
                        span {
                            key: "{group.slug()}",
                            class: "detailed-share-part detailed-group-{group.slug()}",
                            style: "flex-grow: {share}",
                            title: "{group.label()}: {share * 100.0:.0}%",
                        }
                    }
                }
            }
            for (group, rows) in breakdown.groups.iter().filter(|(_, rows)| !rows.is_empty()) {
                div { class: "detailed-group detailed-group-{group.slug()}",
                    div { class: "detailed-group-title",
                        span { class: "detailed-group-swatch" }
                        "{group.label()}"
                    }
                    for (index, row) in rows.iter().enumerate() {
                        div {
                            key: "{index}",
                            class: "detailed-source is-{row.state.slug()}",
                            title: if row.state.note().is_empty() { None } else { Some(row.state.note().to_string()) },
                            span { class: "detailed-source-name",
                                span { class: "detailed-source-label", "{row.label}" }
                                if !row.detail.is_empty() {
                                    span { class: "detailed-source-detail", "{row.detail}" }
                                }
                            }
                            span {
                                class: "detailed-meter",
                                class: if row.value < 0.0 { "is-negative" },
                                span { class: "detailed-meter-fill", style: "width: {width(row.value):.1}%" }
                            }
                            span { class: "detailed-source-value mono", "{ledger_format.render(row.value)}" }
                        }
                    }
                }
            }
            // The fail-loud line. Present only when the rows genuinely fall short of the total,
            // which the reconciliation gate says cannot happen on the corpus — but this surface
            // renders the build in front of the user, not the corpus (Rule 1).
            if let Some(residual) = breakdown.unexplained() {
                div { class: "detailed-source is-unexplained",
                    span { class: "detailed-source-name",
                        span { class: "detailed-source-label", "Unaccounted for" }
                        span { class: "detailed-source-detail",
                            "reached the total with no source reporting it — please report this build"
                        }
                    }
                    span {}
                    span { class: "detailed-source-value mono", "{ledger_format.render(residual)}" }
                }
            }
            div { class: "detailed-total",
                span { "Total" }
                span { class: "mono", "{ledger_format.render(breakdown.total)}" }
            }
        }
    }
}
