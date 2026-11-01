//! Generic modal shell — a fixed full-viewport backdrop plus a centered
//! flex-column card. The rebuild's reusable dialog primitive (the beta
//! `CoH-Sidekick/src/components/modals/Modal.tsx` — beta-only since DEC8), for content-dense UI that a panel-anchored
//! popover would clip: a `position: fixed` card escapes the panel's `overflow`,
//! unlike an `absolute` popover. Structure is preserved from the beta; styling is
//! the rebuild's token vocabulary, not ported Tailwind.
//!
//! Callers mount it above the grid roots — the enhancement picker does this via
//! [`crate::panels::powers::EnhancementPickerHost`]. The rule dates from when the desktop grid
//! placed surfaces with `transform: translate3d`, which contains a `position: fixed` backdrop
//! and clipped a `Modal` to its panel. Surfaces are placed with `left`/`top` now, but each is
//! still an isolated stacking context, so a modal inside one can't rise above its neighbours.
//!
//! Closes on backdrop click, on the header close button, and on Escape. Clicks
//! inside the card are stopped so they never reach the backdrop's close handler.

use dioxus::prelude::*;

/// Card width tier — mirrors the beta `size` prop. The card is always capped at
/// ~90vh tall and manages its own inner scrolling.
// The full tier set is the reusable dialog API; only some are wired to callers yet
// (the enhancement picker uses `Lg`), so the rest are constructed as more beta modals
// are ported.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalSize {
    Sm,
    Md,
    Lg,
    Xl,
    Full,
}

impl ModalSize {
    fn class(self) -> &'static str {
        match self {
            ModalSize::Sm => "modal-card-sm",
            ModalSize::Md => "modal-card-md",
            ModalSize::Lg => "modal-card-lg",
            ModalSize::Xl => "modal-card-xl",
            ModalSize::Full => "modal-card-full",
        }
    }
}

/// The smallest a resizable card can be dragged to: the picker's controls still fit.
const MIN_CARD: (f64, f64) = (560.0, 420.0);

/// A modal dialog. Renders a backdrop + titled card; `children` fill the card
/// body below the header. The caller owns the open/closed state and simply stops
/// rendering `Modal` to dismiss it (matching the beta's `isOpen` gate).
#[component]
pub fn Modal(
    title: String,
    #[props(default = ModalSize::Md)] size: ModalSize,
    on_close: EventHandler<()>,
    /// Every key press and release inside the dialog, for a caller that reacts to a held key.
    /// The backdrop holds focus on open, so a listener inside `children` would miss them.
    #[props(default)]
    on_key: Option<EventHandler<KeyboardEvent>>,
    /// A card the user can resize by its corner grip, holding the dragged `(width, height)`.
    /// The stylesheet's size stands until the first drag; phones show no grip and ignore it.
    #[props(default)]
    resizable: Option<Signal<Option<(f64, f64)>>>,
    children: Element,
) -> Element {
    // The card's rendered size, and the drag in progress: where the pointer and the card
    // started. The card is centred, so a corner follows the pointer at twice its travel.
    let mut rendered = use_signal(|| (0.0_f64, 0.0_f64));
    let mut drag = use_signal(|| None::<((f64, f64), (f64, f64))>);
    let mut live = use_signal(|| None::<(f64, f64)>);
    // A release outside the card can send the backdrop a click; a real click starts with a
    // press on the backdrop, which clears this.
    let mut just_resized = use_signal(|| false);
    let size_style = live()
        .or_else(|| resizable.and_then(|size| size()))
        .map(|(w, h)| format!("--card-w: {w:.0}px; --card-h: {h:.0}px;"))
        .unwrap_or_default();
    let resize_class = if resizable.is_some() {
        "is-resizable"
    } else {
        ""
    };
    rsx! {
        div {
            class: "modal-backdrop",
            tabindex: "-1",
            // Focus the backdrop on mount so it receives the Escape keydown without a
            // document-level listener (the beta uses one; Dioxus focuses the node).
            onmounted: move |evt| async move {
                let _ = evt.set_focus(true).await;
            },
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    on_close.call(());
                } else if let Some(on_key) = on_key {
                    on_key.call(evt);
                }
            },
            onkeyup: move |evt| {
                if let Some(on_key) = on_key {
                    on_key.call(evt);
                }
            },
            // Backdrop click closes; card clicks are stopped below, so this only
            // fires for clicks that land on the dimmed surround.
            onpointerdown: move |_| just_resized.set(false),
            onclick: move |_| {
                if just_resized() {
                    just_resized.set(false);
                } else {
                    on_close.call(());
                }
            },
            div {
                class: "modal-card {size.class()} {resize_class}",
                style: "{size_style}",
                onclick: move |evt| evt.stop_propagation(),
                onresize: move |evt| {
                    if let Ok(size) = evt.get_border_box_size() {
                        rendered.set((size.width, size.height));
                    }
                },
                div { class: "modal-header",
                    h2 { class: "modal-title", "{title}" }
                    button {
                        class: "modal-close",
                        "aria-label": "Close",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                div { class: "modal-content", {children} }
                if let Some(mut stored) = resizable {
                    div {
                        class: "modal-grip",
                        "aria-hidden": "true",
                        title: "Drag to resize",
                        onpointerdown: move |evt: Event<PointerData>| {
                            evt.prevent_default();
                            evt.stop_propagation();
                            let at = evt.client_coordinates();
                            drag.set(Some(((at.x, at.y), rendered())));
                            let id = evt.pointer_id();
                            document::eval(&format!(
                                "try {{ document.querySelector('.modal-grip').setPointerCapture({id}); }} catch (_) {{}}"
                            ));
                        },
                        onpointermove: move |evt: Event<PointerData>| {
                            let Some(((x0, y0), (w0, h0))) = drag() else { return };
                            let at = evt.client_coordinates();
                            live.set(Some((
                                (w0 + 2.0 * (at.x - x0)).max(MIN_CARD.0),
                                (h0 + 2.0 * (at.y - y0)).max(MIN_CARD.1),
                            )));
                        },
                        onpointerup: move |_| {
                            if drag.take().is_some() {
                                just_resized.set(true);
                                // What the card actually shows, after the stylesheet's clamp
                                // to the window, so a reopen doesn't start past the edge.
                                if live.take().is_some() {
                                    stored.set(Some(rendered()));
                                }
                            }
                        },
                        onpointercancel: move |_| {
                            drag.set(None);
                            live.set(None);
                        },
                    }
                }
            }
        }
    }
}
