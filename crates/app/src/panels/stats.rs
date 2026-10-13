//! Stats — the whole-build dashboard: [`coh_math::recalculate`] over the live `CharacterState`,
//! projected to [`coh_math::CharacterStats`] (the single capped truth, D4). Reads
//! `CalculatedTotals` only; the per-power effect detail lives in the Info panel.
//!
//! **One component, however many panels.** Every dashboard panel is a
//! [`PanelKind::Dashboard`](crate::grid::PanelKind::Dashboard) carrying nothing but an id, so it
//! can be dragged, resized and placed on its own — a build that lives or dies on three numbers
//! can put those three numbers in one small surface and leave everything else off the grid.
//! They all read the one [`BuildTotals`] context, so splitting the dashboard across panels costs
//! no extra recalculation: compute once, render many.
//!
//! This used to be eight components over eight fixed groups, and the eight were a consequence
//! of where membership was decided rather than a judgement about how a build reads: a stat's
//! panel was a const field on its registry entry, so every group the registry named had to be a
//! surface, whether the user wanted it or not.
//!
//! No panel here knows a stat. Which rows a panel holds and in what order is the user's, held
//! in [`crate::panels::dashboards`]; what each row is called, its colour, its unit and how to
//! read it off the engine are the registry's. This component owns neither — it looks up ids and
//! draws them.
//!
//! **Colour never carries meaning alone.** Hue says which stat FAMILY a number belongs to, so a
//! build's defense numbers can be found at a glance; it says nothing about good or bad. The two
//! things that are good-or-bad — a value at its ceiling, and a total the Rule of 5 shrank — are
//! drawn as a dotted underline and a warning ring, both legible with no colour vision at all.
//!
//! Fail-loud errors do NOT live here. `bonuses.errors` surface in the shell's status strip
//! ([`crate::shell::StatusStrip`]), outside every panel, because an error routed into a panel is
//! only as visible as that panel's placement — and Rule 1 wants it visible unconditionally.

use crate::grid::model::DashboardId;
use crate::panels::dashboards::DashboardConfig;
use crate::panels::detailed_totals::{breakdowns_for_stat, is_expandable, KeyBreakdownBlock};
use crate::panels::stat_registry;
use crate::panels::stats_config::StatsConfigOpen;
use crate::shell::Db;
use crate::tracked_stats::TrackedStats;
use coh_math::CalculatedTotals;
use dioxus::prelude::*;

/// Context handle for the one whole-build recompute, provided at the shell root over the loaded
/// dataset + live build. Every stat panel and the power-card slot tooltip (set-bonus Rule-of-5
/// provenance) read it, so `recalculate` — the full pass pipeline — runs once per edit, never
/// once per reader. A newtype so it never collides with another `Memo<_>` in context.
#[derive(Clone, Copy, PartialEq)]
pub struct BuildTotals(pub Memo<CalculatedTotals>);

/// One dashboard panel's body: the stats the user put in it, in the order they put them,
/// each coloured by family and marked with whichever status cues apply. The panel's *name* is
/// its own title in the grid chrome, so it is never repeated inside the body.
///
/// Takes a [`DashboardId`] where it used to take a `StatSection`, and the difference is the
/// whole change: a section decided its own membership at compile time, so this component read
/// the registry and filtered by a visible flag. A panel's membership is the roster's to state,
/// so it reads the roster — which is also what makes the ORDER the user's, a thing a filter
/// over a static list could not express.
#[component]
pub fn StatGroup(panel: DashboardId) -> Element {
    let totals = use_context::<BuildTotals>().0;
    let dashboards = use_context::<DashboardConfig>().0;
    let mut config_open = use_context::<StatsConfigOpen>().0;
    let tracked = use_context::<TrackedStats>();
    // Absent in a popped-out window, which has no host to draw the card; its rows keep the
    // native `title` text instead.
    let hover = try_use_context::<StatHover>();

    // Both reads resolve to owned values before `rsx!`, so no borrow of the shared totals memo
    // or the config is held across the render (keep logic above `rsx!`).
    let rows: Vec<_> = {
        let totals = totals.read();
        let dashboards = dashboards.read();
        dashboards
            .stats_in(panel)
            .iter()
            .filter_map(|id| stat_registry::by_id(id))
            .map(|stat| (stat, stat.resolve(&totals), tracked.contains(stat.id)))
            .collect()
    };

    rsx! {
        div { class: "stats",
            if rows.is_empty() {
                // An empty panel is no longer an anomaly — it is what a panel the user just
                // made looks like before they fill it — so this is a prompt rather than an
                // explanation. It still opens the one thing that fills it, scrolled to this
                // panel's own card.
                button {
                    class: "stats-empty",
                    r#type: "button",
                    onclick: move |_| {
                        config_open.set(Some(
                            crate::panels::stats_config::OrganizerTarget::Panel(panel),
                        ))
                    },
                    "Empty — add stats"
                }
            }
            for (stat, resolved, is_tracked) in rows {
                // The whole row is the switch, as in the beta: a stat is tracked to chase it with
                // set bonuses, and the picker marks the sets that would move it.
                div {
                    class: "stat-row",
                    class: if is_tracked { "is-tracked" },
                    role: "button",
                    tabindex: "0",
                    aria_pressed: "{is_tracked}",
                    onclick: move |_| tracked.toggle(stat.id),
                    // A mouse only: a tap fires no leave until the next tap elsewhere, which would
                    // leave the card standing over the phone's dashboard.
                    onpointerenter: move |evt: PointerEvent| {
                        if let Some(hover) = hover.filter(|_| evt.pointer_type() == "mouse") {
                            hover.enter(stat.id, &evt);
                        }
                    },
                    onpointerleave: move |_| {
                        if let Some(hover) = hover {
                            hover.leave();
                        }
                    },
                    onkeydown: move |evt: KeyboardEvent| {
                        if matches!(evt.key(), Key::Enter) || evt.key() == Key::Character(" ".into()) {
                            evt.prevent_default();
                            tracked.toggle(stat.id);
                        }
                    },
                    // The label is the half a narrow surface truncates (see `.stat-label`), and
                    // CSS cannot tell us whether it did — so the full name is carried
                    // unconditionally: in the hover card, or here where there is none.
                    span {
                        class: "stat-label",
                        title: if hover.is_none() { Some(track_hint(stat.label, is_tracked)) } else { None },
                        "{stat.label}"
                    }
                    // The hairline between the name and the number. A real element rather than a
                    // pseudo on the label, because `.stat-label` earns its ellipsis from being a
                    // plain text box — making it a flex container to hang a leader off would
                    // cost the truncation that keeps a long stat name out of the value column.
                    span { class: "leader" }
                    span {
                        class: "stat-value mono",
                        class: if resolved.at_cap { "is-at-cap" },
                        class: if resolved.rule_of_five_capped { "is-rule-of-five-capped" },
                        class: if resolved.simulated { "is-simulated" },
                        style: "--stat-hue: {stat.family.token()};",
                        // Both cues are decoration; the title (or the hover card) is what
                        // carries them to anyone who does not read a hue as meaning anything.
                        title: if hover.is_none() { Some(cue_description(&resolved)) } else { None },
                        "{resolved.text}"
                        // A text mark, not only a hue: the whole point is that a SCREENSHOT of
                        // this dashboard cannot pass as the build's own numbers, and a screenshot
                        // carries no tooltip and no colour vocabulary.
                        if resolved.simulated {
                            span {
                                class: "stat-simulated",
                                title: if hover.is_none() { Some("Includes a simulated team buff") } else { None },
                                "sim"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// What clicking a row does, named with the stat so a truncated label still reads in full.
fn track_hint(label: &str, is_tracked: bool) -> String {
    if is_tracked {
        format!("{label} — tracked: the picker marks sets that boost it. Click to untrack.")
    } else {
        format!("{label} — click to track: the picker marks sets that boost it.")
    }
}

/// What the row's tooltip says: the stat's own note (what the number was projected from, for the
/// stats that show a projection) followed by what the status cues mean. Empty when the value
/// carries neither, so no row grows a pointless empty tooltip.
fn cue_description(resolved: &stat_registry::ResolvedStat) -> String {
    let mut notes = Vec::new();
    if !resolved.note.is_empty() {
        notes.push(resolved.note.as_str());
    }
    if resolved.at_cap {
        notes.push("At or above its cap.");
    }
    if resolved.rule_of_five_capped {
        notes.push("A contributing set bonus was rejected by the Rule of 5.");
    }
    if resolved.simulated {
        notes.push("SIMULATED — includes a what-if team buff, not the build's own number.");
    }
    notes.join(" ")
}

// ============================================================
// The hover card.
// ============================================================

/// The dashboard row under the pointer, and the viewport point (the cursor at mouse-enter) the
/// card hangs from.
#[derive(Clone, PartialEq)]
pub struct StatHoverTarget {
    pub stat_id: &'static str,
    pub anchor_x: f64,
    pub anchor_y: f64,
}

/// Context handle for the hovered dashboard row, provided at the shell root. `None` = the
/// pointer is on no row. A newtype so it never collides with another `Signal<Option<_>>`.
#[derive(Clone, Copy)]
pub struct StatHover(pub Signal<Option<StatHoverTarget>>);

impl StatHover {
    pub fn enter(self, stat_id: &'static str, evt: &Event<PointerData>) {
        let mut hover = self.0;
        let point = evt.client_coordinates();
        hover.set(Some(StatHoverTarget {
            stat_id,
            anchor_x: point.x,
            anchor_y: point.y,
        }));
    }

    pub fn leave(self) {
        let mut hover = self.0;
        hover.set(None);
    }
}

/// Keep the card inside the window, as the power card does
/// ([`crate::power_tooltip`]'s `KEEP_POWER_TOOLTIP_ON_SCREEN`).
const KEEP_STAT_TOOLTIP_ON_SCREEN: &str = "\
document.querySelectorAll('.stat-tooltip').forEach(function (tip) {\
  var z = parseFloat(getComputedStyle(document.documentElement).zoom) || 1;\
  var rect = tip.getBoundingClientRect();\
  var ax = parseFloat(tip.style.left), ay = parseFloat(tip.style.top);\
  var offset = 14, margin = 8;\
  var x = ax + offset, y = ay + offset;\
  if (x + rect.width > window.innerWidth - margin) { x = ax - offset - rect.width; }\
  if (x < margin) { x = Math.max(margin, window.innerWidth - margin - rect.width); }\
  if (y + rect.height > window.innerHeight - margin) { y = ay - offset - rect.height; }\
  if (y < margin) { y = Math.max(margin, window.innerHeight - margin - rect.height); }\
  tip.style.transform = 'translate(' + (x / z - ax) + 'px, ' + (y / z - ay) + 'px)';\
});";

/// The hovered row's sources, hosted at the shell root so a grid surface's `transform` cannot
/// clip it. The beta's dashboard tooltip, drawn with the Detailed Totals sheet's own per-key
/// block so the two surfaces cannot disagree about a row.
///
/// The card mounts at once and stays transparent for a beat (`.stat-tooltip` in app.css), so a
/// pointer crossing the dashboard does not flash a card over every row it passes.
#[component]
pub fn StatTooltipHost(database: Option<Db>) -> Element {
    let hover = use_context::<StatHover>().0;
    let totals = use_context::<BuildTotals>().0;
    let tracked = use_context::<TrackedStats>();

    use_effect(move || {
        if hover.read().is_some() {
            document::eval(KEEP_STAT_TOOLTIP_ON_SCREEN);
        }
    });

    let Some(target) = hover() else {
        return rsx! {};
    };
    let Some(stat) = stat_registry::by_id(target.stat_id) else {
        return rsx! {};
    };
    let (resolved, breakdowns) = {
        let totals = totals.read();
        (
            stat.resolve(&totals),
            breakdowns_for_stat(&totals, database.as_ref(), stat),
        )
    };
    let cues = cue_description(&resolved);
    let expandable = is_expandable(&breakdowns);
    let hint = if tracked.contains(stat.id) {
        "Tracked: the picker marks sets that boost it. Click to untrack."
    } else {
        "Click to track: the picker marks sets that boost it."
    };

    rsx! {
        div {
            // Keyed by stat so moving to the next row remounts the card and restarts its delay.
            key: "{stat.id}",
            class: "stat-tooltip",
            role: "tooltip",
            style: "left: {target.anchor_x}px; top: {target.anchor_y}px; --stat-hue: {stat.family.token()};",
            div { class: "stat-tooltip-head",
                span { "{stat.label}" }
                span { class: "mono", "{resolved.text}" }
            }
            if !cues.is_empty() {
                div { class: "stat-tooltip-note", "{cues}" }
            }
            if expandable {
                for breakdown in breakdowns {
                    KeyBreakdownBlock {
                        key: "{breakdown.key}",
                        breakdown: breakdown.clone(),
                        ledger_format: stat.ledger_format,
                        show_key_heading: stat.ledger_keys.len() > 1,
                    }
                }
            } else if stat.ledger_keys.is_empty() {
                div { class: "stat-tooltip-note", "Worked out from other stats rather than added up from sources." }
            } else {
                div { class: "stat-tooltip-note", "Nothing in this build adds to it." }
            }
            div { class: "stat-tooltip-hint", "{hint}" }
        }
    }
}
