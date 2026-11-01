//! How far into Enhancement Diversification a power's slotting is, on screen: a small mark
//! after each power's slots, coloured by its deepest stage, a card on hovering it, and the same
//! breakdown in the Info panel for phones, which have no hover.
//!
//! Every number is the engine's per-power breakdown ([`coh_math::ed_breakdown`]) carried on the
//! power's projection, so the card, the panel and the totals read one source.

use crate::panels::stats::BuildTotals;
use crate::shell::PowerRef;
use coh_math::ed_breakdown::AspectBreakdown;
use dioxus::prelude::*;

/// The power whose ED mark is under the pointer, and where the pointer entered it.
#[derive(Clone, PartialEq)]
pub struct EdHoverTarget {
    pub power: PowerRef,
    pub anchor_x: f64,
    pub anchor_y: f64,
}

/// Context handle for the hovered ED mark, provided at the shell root.
#[derive(Clone, Copy)]
pub struct EdHover(pub Signal<Option<EdHoverTarget>>);

/// What the build's projection says about one power's ED. Empty when nothing is slotted.
pub fn rows_for(totals: &coh_math::CalculatedTotals, power: &PowerRef) -> Vec<AspectBreakdown> {
    totals
        .power_projection
        .iter()
        .find(|p| p.power_set == power.powerset_id && p.power_internal_name == power.power)
        .map(|p| p.enhancement_breakdown.clone())
        .unwrap_or_default()
}

/// The deepest stage any aspect reached, which colours the mark. `None` with nothing slotted.
pub fn deepest_stage(rows: &[AspectBreakdown]) -> Option<u8> {
    rows.iter().map(|row| row.stage).max()
}

/// The icon's colour: neutral, only full value or not. A warning colour on most cards would
/// read as a build full of mistakes, since sets routinely pass a threshold.
fn mark_class(stage: u8) -> &'static str {
    if stage == 0 {
        "is-room"
    } else {
        "is-diminished"
    }
}

/// The card's bar keeps the heat: amber on the steep part, red past the last step.
fn bar_class(stage: u8) -> &'static str {
    match stage {
        0 | 1 => "is-room",
        2 => "is-steep",
        _ => "is-capped",
    }
}

/// What a stage means to a player. Kept plain: sets routinely push an aspect past a threshold,
/// so it is a fact about the power, not a fault.
fn stage_words(stage: u8) -> &'static str {
    if stage == 0 {
        "Full value"
    } else {
        "Diminishing returns in effect"
    }
}

/// An aspect as a reader names it: its enhancement category, spaced ("Endurance Reduction").
/// Absorb has no category of its own (it rides on Healing), so it keeps its own name.
fn aspect_label(aspect: &str) -> String {
    if aspect == "absorb" {
        return "Absorb".to_string();
    }
    let Some(category) = coh_math::enhancement::enhancement_type_of(aspect) else {
        let mut chars = aspect.chars();
        return chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + chars.as_str()
        });
    };
    let mut label = String::new();
    let mut previous: Option<char> = None;
    for ch in category.chars() {
        if ch.is_uppercase() && previous.is_some_and(|p| p.is_lowercase()) {
            label.push(' ');
        }
        label.push(ch);
        previous = Some(ch);
    }
    label
}

fn percent(fraction: f64) -> String {
    let value = fraction * 100.0;
    if (value - value.round()).abs() < 0.05 {
        format!("{value:.0}%")
    } else {
        format!("{value:.1}%")
    }
}

/// The mark after a power's slots. Hovering it opens the card ([`EdTipHost`]).
#[component]
pub fn EdMark(power: PowerRef, stage: u8) -> Element {
    let mut hover = use_context::<EdHover>().0;
    rsx! {
        span {
            class: "ed-mark {mark_class(stage)}",
            "aria-label": "Enhancement Diversification: {stage_words(stage)}",
            onmouseenter: {
                let power = power.clone();
                move |evt: Event<MouseData>| {
                    let point = evt.client_coordinates();
                    hover.set(Some(EdHoverTarget {
                        power: power.clone(),
                        anchor_x: point.x,
                        anchor_y: point.y,
                    }));
                }
            },
            onmouseleave: move |_| hover.set(None),
            svg {
                class: "ed-mark__icon",
                view_box: "0 0 16 16",
                "aria-hidden": "true",
                circle { cx: "8", cy: "8", r: "6.5" }
                path { d: "M8 7v4.5M8 4.6v.1" }
            }
        }
    }
}

/// The card for the hovered mark, hosted at the shell root so no panel clips it.
#[component]
pub fn EdTipHost() -> Element {
    let hover = use_context::<EdHover>().0;
    let totals = use_context::<BuildTotals>().0;
    use_effect(move || {
        if hover.read().is_some() {
            document::eval(crate::power_tooltip::KEEP_POWER_TOOLTIP_ON_SCREEN);
        }
    });
    let Some(target) = hover() else {
        return rsx! {};
    };
    let rows = rows_for(&totals.read(), &target.power);
    if rows.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            class: "power-tooltip",
            style: "left: {target.anchor_x}px; top: {target.anchor_y}px;",
            div { class: "power-tooltip__card ed-tip",
                EdBreakdown { rows }
            }
        }
    }
}

/// One line per aspect: a bar with the ED steps marked, slotted against what counts, and how
/// much ED takes.
#[component]
pub fn EdBreakdown(rows: Vec<AspectBreakdown>) -> Element {
    rsx! {
        div { class: "ed-breakdown",
            for row in rows.iter() {
                EdRow { key: "{row.aspect}", row: row.clone() }
            }
        }
    }
}

#[component]
fn EdRow(row: AspectBreakdown) -> Element {
    let total = row.slotted + row.alpha;
    // The bar's scale: a little past the last step, or past the total when it goes further.
    let scale = (row.thresholds[2] * 1.25)
        .max(total * 1.05)
        .max(f64::EPSILON);
    let at = |value: f64| (value / scale * 100.0).clamp(0.0, 100.0);
    let alpha_note =
        (row.alpha > 0.0).then(|| format!(" (Alpha adds {} before ED)", percent(row.alpha)));
    rsx! {
        div {
            class: "ed-row {bar_class(row.stage)}",
            title: "{aspect_label(row.aspect)}: {stage_words(row.stage)}",
            div { class: "ed-row__head",
                span { class: "ed-row__name", {aspect_label(row.aspect)} }
                span { class: "ed-row__values",
                    "{percent(row.slotted)} slotted → "
                    span { class: "ed-row__counted", "{percent(row.counted)}" }
                    " counts"
                }
            }
            div { class: "ed-row__bar",
                span { class: "ed-row__fill", style: "width: {at(total)}%;" }
                for (index , step) in row.thresholds.iter().enumerate() {
                    span {
                        key: "{index}",
                        class: "ed-row__step",
                        style: "left: {at(*step)}%;",
                    }
                }
            }
            div { class: "ed-row__foot",
                span {
                    {stage_words(row.stage)}
                    if row.lost > 0.0 {
                        ", {percent(row.lost)} lost"
                    }
                }
            }
            if let Some(note) = alpha_note {
                div { class: "ed-row__alpha", "{note}" }
            }
        }
    }
}

/// The enhancement picker's strip: each aspect's after-ED value now, and an arrow to what the
/// marks and the hovered item would make it. An aspect only one side has reads 0% on the other,
/// so a piece that adds Recharge, or a replaced one that took Accuracy away, shows as such.
#[component]
pub fn EdPreviewStrip(now: Vec<AspectBreakdown>, after: Vec<AspectBreakdown>) -> Element {
    let mut aspects: Vec<&'static str> = now.iter().map(|row| row.aspect).collect();
    for row in &after {
        if !aspects.contains(&row.aspect) {
            aspects.push(row.aspect);
        }
    }
    let find = |rows: &[AspectBreakdown], aspect: &str| {
        rows.iter().find(|row| row.aspect == aspect).cloned()
    };
    // The strip keeps its tallest height at this width. Shrinking back when the pointer left a
    // piece would grow the list, slide the piece back under the pointer, and loop.
    let mut floor = use_signal(|| (0.0_f64, 0.0_f64));
    let (_, floor_px) = floor();
    rsx! {
        div {
            class: "picker-preview",
            style: "min-height: {floor_px}px;",
            "aria-live": "polite",
            onresize: move |evt| {
                let Ok(size) = evt.get_border_box_size() else { return };
                let (width, tallest) = floor();
                if (size.width - width).abs() > 0.5 {
                    // A new width wraps differently: let go and measure afresh.
                    floor.set((size.width, 0.0));
                } else if size.height > tallest + 0.5 {
                    floor.set((width, size.height));
                }
            },
            span { class: "picker-preview__title", "Enhancement" }
            if aspects.is_empty() {
                span { class: "picker-preview__empty", "none yet" }
            }
            for aspect in aspects {
                {
                    let was = find(&now, aspect);
                    let will = find(&after, aspect);
                    let was_value = was.as_ref().map_or(0.0, |row| row.counted);
                    let will_value = will.as_ref().map_or(0.0, |row| row.counted);
                    let stage = will.as_ref().or(was.as_ref()).map_or(0, |row| row.stage);
                    let label = aspect_label(aspect);
                    let short = crate::naming::stat_abbreviation(&label).to_string();
                    rsx! {
                        span {
                            key: "{aspect}",
                            class: "picker-preview__aspect",
                            title: "{label}: {stage_words(stage)}",
                            span { class: "picker-preview__name", "{short}" }
                            " {percent(was_value)}"
                            if (will_value - was_value).abs() > 0.000_5 {
                                " → "
                                span { class: "picker-preview__after {bar_class(stage)}",
                                    "{percent(will_value)}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
