//! The floating power card (beta `PowerInfoTooltip`): an opt-in card beside the pointer that
//! shows the hovered power's stats, so a power can be read without the Info panel in view.
//! Holding Shift pins the current card, and the next power hovered shows beside it — a
//! side-by-side comparison.
//!
//! An Options preference kept across reloads like [`crate::proc_potential_store`]. Off by
//! default, the beta's default: the Info panel already follows the pointer, and a second copy
//! of it over the build is clutter to anyone who did not ask for it.
//!
//! Hover here is a separate signal from [`crate::shell::PowerSelection`]. The selection is
//! sticky — the Info panel keeps the last power after the pointer leaves — but a card that
//! outlived the pointer would sit over whatever the player moved on to.

use crate::build_session::BuildSession;
use crate::panels::info::{slotted_proc_rows, PowerTooltipCard};
use crate::panels::stats::BuildTotals;
use crate::shell::{Db, PowerRef};
use crate::view::power_view::resolve_selected_view;
use dioxus::prelude::*;

const KEY_POWER_TOOLTIP: &str = "sk-power-tooltip";

/// Whether the floating card is wanted. Provided once at the shell root.
#[derive(Clone, Copy)]
pub struct PowerTooltipEnabled(pub Signal<bool>);

/// The power under the pointer, and the viewport point (the cursor at mouse-enter) the card
/// hangs from.
#[derive(Clone, PartialEq)]
pub struct PowerHoverTarget {
    pub power: PowerRef,
    pub anchor_x: f64,
    pub anchor_y: f64,
}

/// Context handle for the hovered power. `None` = the pointer is on no power. A newtype so it
/// never collides with another `Signal<Option<_>>` in context.
#[derive(Clone, Copy)]
pub struct PowerHover(pub Signal<Option<PowerHoverTarget>>);

impl PowerHover {
    /// The pointer entered a power's row or card.
    pub fn enter(self, power: PowerRef, evt: &Event<MouseData>) {
        let mut hover = self.0;
        let point = evt.client_coordinates();
        hover.set(Some(PowerHoverTarget {
            power,
            anchor_x: point.x,
            anchor_y: point.y,
        }));
    }

    /// The pointer left it.
    pub fn leave(self) {
        let mut hover = self.0;
        hover.set(None);
    }
}

/// Persist the setting (fire-and-forget, like [`crate::proc_potential_store::persist`]).
pub fn persist(enabled: bool) {
    let js = format!(
        "try {{ localStorage.setItem({KEY_POWER_TOOLTIP:?}, {}); }} catch (_) {{}}",
        if enabled { "\"true\"" } else { "\"false\"" }
    );
    crate::storage::commit(js);
}

/// The persisted setting; `None` (missing, or anything we didn't write) keeps the default.
pub async fn load() -> Option<bool> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_POWER_TOOLTIP:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    match value.as_str()? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Shift down and up, posted back to Rust. Skips edit fields like the shell's own shortcuts,
/// and treats losing the window as letting go: a Shift released in another window never
/// reaches this one, and the pin would otherwise stay up until the next press.
const SHIFT_KEYS: &str = "\
document.addEventListener('keydown', (e) => {\
  if (e.key !== 'Shift' || e.repeat) return;\
  const t = e.target;\
  const tag = t && t.tagName;\
  if (t && (t.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT')) return;\
  dioxus.send('down');\
});\
document.addEventListener('keyup', (e) => { if (e.key === 'Shift') dioxus.send('up'); });\
window.addEventListener('blur', () => dioxus.send('up'));";

/// Keep the card inside the window, the same way the slot tooltip does
/// ([`crate::panels::powers`]' `KEEP_SLOT_TOOLTIP_ON_SCREEN`): beside the cursor, flipped to
/// the other side when it would run off the right or bottom edge, and pinned inside an 8px
/// margin when neither side fits. Placed in window pixels and divided by the UI scale's `zoom`.
const KEEP_POWER_TOOLTIP_ON_SCREEN: &str = "\
document.querySelectorAll('.power-tooltip').forEach(function (tip) {\
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

/// The one floating power card, hosted at the shell root outside both layout roots for the
/// same reason as the slot tooltip: a free-grid surface's `transform` would contain and clip a
/// `position: fixed` card drawn inside it. Renders nothing while the option is off.
///
/// While a slot's own tooltip is up the live card steps aside, since the pointer is on the
/// piece rather than the power. A pinned card stays until Shift is released.
#[component]
pub fn PowerTooltipHost(database: Db) -> Element {
    let enabled = use_context::<PowerTooltipEnabled>().0;
    let hover = use_context::<PowerHover>().0;
    let slot_tip = use_context::<crate::panels::powers::SlotTooltip>().0;
    let session = use_context::<BuildSession>();
    let totals = use_context::<BuildTotals>().0;
    let mut pinned = use_signal(|| Option::<PowerHoverTarget>::None);

    use_future(move || async move {
        let mut eval = document::eval(SHIFT_KEYS);
        while let Ok(msg) = eval.recv::<String>().await {
            match msg.as_str() {
                "down" if *enabled.peek() && pinned.peek().is_none() => {
                    let live = hover.peek().clone();
                    if live.is_some() && slot_tip.peek().is_none() {
                        pinned.set(live);
                    }
                }
                "up" => pinned.set(None),
                _ => {}
            }
        }
    });

    // Re-placed whenever what the card shows changes, after it has laid out at its new size.
    use_effect(move || {
        if enabled() && (hover.read().is_some() || pinned.read().is_some()) {
            document::eval(KEEP_POWER_TOOLTIP_ON_SCREEN);
        }
    });

    // Each card is the Info panel's own view and procs for that power, so a number reads the
    // same in the card as in the panel. Neither memo reads the hover until the option is on.
    let card_for = move |database: &Db, target: Option<PowerHoverTarget>| {
        let target = target?;
        let build = session.build.read();
        let totals = totals.read();
        let selection = Some(target.power.clone());
        let view = resolve_selected_view(database, &build, &totals, &selection)?.ok()?;
        let procs = slotted_proc_rows(database, &build, &totals, &target.power);
        Some((view, procs))
    };
    let live_card = use_memo(use_reactive!(|database| {
        if !enabled() {
            return None;
        }
        card_for(&database, hover())
    }));
    let pinned_card = use_memo(use_reactive!(|database| {
        if !enabled() {
            return None;
        }
        card_for(&database, pinned())
    }));

    if !enabled() {
        return rsx! {};
    }
    let live = hover();
    let pin = pinned();
    // Hidden over a slot, and when it would only repeat the pinned card.
    let show_live = live.as_ref().is_some_and(|live| {
        slot_tip.read().is_none() && pin.as_ref().is_none_or(|pin| pin.power != live.power)
    });
    let anchor = match (&pin, &live) {
        (Some(pin), _) => pin,
        (None, Some(live)) if show_live => live,
        _ => return rsx! {},
    };

    rsx! {
        div {
            class: "power-tooltip",
            style: "left: {anchor.anchor_x}px; top: {anchor.anchor_y}px;",
            if let Some((view, procs)) = pinned_card() {
                div { class: "power-tooltip__card is-pinned",
                    PowerTooltipCard { view, slotted_procs: procs }
                    div { class: "power-tooltip__note", "Pinned — release Shift to drop it" }
                }
            }
            if show_live {
                if let Some((view, procs)) = live_card() {
                    div { class: "power-tooltip__card",
                        PowerTooltipCard { view, slotted_procs: procs }
                        if pin.is_none() {
                            div { class: "power-tooltip__note", "Hold Shift to pin this card and compare" }
                        }
                    }
                }
            }
        }
    }
}
