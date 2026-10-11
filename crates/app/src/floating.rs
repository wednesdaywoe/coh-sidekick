//! A floating panel — the [`crate::modal::Modal`] card without the backdrop. It sits over the
//! grid while the build behind it stays live and clickable, moves by its header, resizes from
//! its corner, and comes back where it was left.
//!
//! For a tool worth keeping open while editing: Detailed Totals is the first, because it is the
//! Mids "View Totals" window and players arriving from Mids expect it to sit beside the build.
//!
//! Mounted at the shell root like every modal host, for the reason [`crate::modal`] gives: each
//! grid surface is its own stacking context, so a panel inside one could not rise above the rest.
//!
//! The drag copies the grid's ([`crate::grid::view`]): a full-viewport layer exists only while a
//! gesture is in flight and owns every pointer move, so a fast drag that outruns the header
//! keeps working, and coordinates are raw client pixels the same way the grid's are.
//!
//! Below 900px there is no room to float, and app.css pins the card to the screen instead. The
//! gestures are switched off there too, so a phone tap on the header never saves a position the
//! desktop layout would then open at.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// The narrowest and shortest a panel can be dragged to — enough for the header and a column.
const MIN_W: f64 = 360.0;
const MIN_H: f64 = 220.0;
/// How much of a panel must stay on screen horizontally, so its header can always be grabbed.
const KEEP_VISIBLE: f64 = 120.0;
/// Below this width the CSS pins the card and the gestures are off — the app's mobile break.
const FLOAT_MIN_VIEWPORT: f64 = 900.0;
/// Used until the viewport has been measured; only ever seen if that read fails.
const FALLBACK_VIEWPORT: (f64, f64) = (1280.0, 800.0);

/// Where a panel sits and how big it is, in client pixels.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct PanelRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PanelRect {
    /// Centred in the viewport at the preferred size, shrunk to fit with a margin.
    pub fn centered((w, h): (f64, f64), (vw, vh): (f64, f64)) -> PanelRect {
        let w = w.min(vw - 32.0);
        let h = h.min(vh - 32.0);
        PanelRect {
            x: (vw - w) / 2.0,
            y: (vh - h) / 2.0,
            w,
            h,
        }
        .fit((vw, vh))
    }

    /// Clamped so the header stays reachable: never above the top edge, never so far off a side
    /// that less than [`KEEP_VISIBLE`] of it shows, and never larger than the viewport. A saved
    /// position from a bigger window is pulled back on screen by this rather than lost.
    pub fn fit(self, (vw, vh): (f64, f64)) -> PanelRect {
        let w = self.w.clamp(MIN_W, vw.max(MIN_W));
        let h = self.h.clamp(MIN_H, vh.max(MIN_H));
        PanelRect {
            x: self.x.clamp(KEEP_VISIBLE - w, (vw - KEEP_VISIBLE).max(0.0)),
            y: self.y.clamp(0.0, (vh - 40.0).max(0.0)),
            w,
            h,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    Move,
    Resize,
}

#[derive(Clone, Copy, PartialEq)]
struct Drag {
    gesture: Gesture,
    start: PanelRect,
    origin: (f64, f64),
}

impl Drag {
    fn apply(self, (px, py): (f64, f64), viewport: (f64, f64)) -> PanelRect {
        let (dx, dy) = (px - self.origin.0, py - self.origin.1);
        let start = self.start;
        let next = match self.gesture {
            Gesture::Move => PanelRect {
                x: start.x + dx,
                y: start.y + dy,
                ..start
            },
            Gesture::Resize => PanelRect {
                w: start.w + dx,
                h: start.h + dy,
                ..start
            },
        };
        next.fit(viewport)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "schema_version", content = "data")]
enum StoredRect {
    #[serde(rename = "1")]
    V1(PanelRect),
}

fn storage_key(id: &str) -> String {
    format!("sk-float-{id}")
}

fn persist(id: &str, rect: PanelRect) {
    let Ok(json) = serde_json::to_string(&StoredRect::V1(rect)) else {
        return;
    };
    let key = storage_key(id);
    crate::storage::commit(format!(
        "try {{ localStorage.setItem({key:?}, {json:?}); }} catch (_) {{}}"
    ));
}

async fn load(id: &str) -> Option<PanelRect> {
    let key = storage_key(id);
    let value = document::eval(&format!(
        "try {{ return localStorage.getItem({key:?}); }} catch (_) {{ return null; }}"
    ))
    .await
    .ok()?;
    let StoredRect::V1(rect) = serde_json::from_str(value.as_str()?).ok()?;
    Some(rect)
}

async fn measure_viewport() -> Option<(f64, f64)> {
    let value = document::eval("return [window.innerWidth, window.innerHeight];")
        .await
        .ok()?;
    let pair = value.as_array()?;
    Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?))
}

/// A floating panel. Like [`crate::modal::Modal`], the caller owns the open state and stops
/// rendering this to close it. `id` names the saved position, so it must be unique per tool.
///
/// Escape closes it while focus is inside — it takes focus when it opens, and gives it up the
/// moment the user clicks the build, which is what makes it a panel rather than a dialog.
#[component]
pub fn FloatingPanel(
    id: &'static str,
    title: String,
    /// Opening size in pixels before the user has moved or resized it.
    default_size: (f64, f64),
    on_close: EventHandler<()>,
    children: Element,
) -> Element {
    let mut rect = use_signal(|| Option::<PanelRect>::None);
    let mut viewport = use_signal(|| FALLBACK_VIEWPORT);
    let drag = use_signal(|| Option::<Drag>::None);
    let mut card = use_signal(|| Option::<std::rc::Rc<MountedData>>::None);

    // Nothing renders until the saved position is read, so the panel never opens centred and
    // then jumps to where it was left.
    use_future(move || async move {
        let measured = measure_viewport().await.unwrap_or(FALLBACK_VIEWPORT);
        viewport.set(measured);
        let placed = match load(id).await {
            Some(saved) => saved.fit(measured),
            None => PanelRect::centered(default_size, measured),
        };
        rect.set(Some(placed));
    });

    let Some(current) = rect() else {
        return rsx! {};
    };

    // Starts a gesture. The viewport is re-read on every grab rather than watched, because a
    // window resized since the panel opened is the only time it changes, and the grab is the
    // only time the clamp needs it.
    let begin = move |evt: PointerEvent, gesture: Gesture| {
        let (mut drag, mut viewport) = (drag, viewport);
        if viewport.peek().0 < FLOAT_MIN_VIEWPORT {
            return;
        }
        // Stops the press from starting a text selection that follows the drag. It also stops
        // the press from focusing the panel, so focus is handed over here instead — otherwise
        // Escape would not reach a panel the user had just grabbed.
        evt.prevent_default();
        if let Some(card) = card.peek().clone() {
            spawn(async move {
                let _ = card.set_focus(true).await;
            });
        }
        let point = evt.client_coordinates();
        drag.set(Some(Drag {
            gesture,
            start: current,
            origin: (point.x, point.y),
        }));
        spawn(async move {
            if let Some(measured) = measure_viewport().await {
                viewport.set(measured);
            }
        });
    };

    let finish = move || {
        let mut drag = drag;
        if drag.peek().is_some() {
            drag.set(None);
            if let Some(placed) = *rect.peek() {
                persist(id, placed);
            }
        }
    };

    let gesture_class = match drag() {
        Some(Drag {
            gesture: Gesture::Resize,
            ..
        }) => "floating-drag-layer--resize",
        _ => "",
    };

    rsx! {
        div {
            class: "floating-panel",
            role: "dialog",
            "aria-label": "{title}",
            tabindex: "-1",
            style: "left: {current.x}px; top: {current.y}px; width: {current.w}px; height: {current.h}px;",
            onmounted: move |evt| async move {
                let mounted = evt.data();
                card.set(Some(mounted.clone()));
                let _ = mounted.set_focus(true).await;
            },
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    on_close.call(());
                }
            },
            div {
                class: "floating-header",
                onpointerdown: move |evt| begin(evt, Gesture::Move),
                h2 { class: "modal-title", "{title}" }
                button {
                    class: "modal-close",
                    "aria-label": "Close",
                    // A press on the close button is a click, not the start of a drag.
                    onpointerdown: move |evt| evt.stop_propagation(),
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }
            div { class: "floating-content", {children} }
            div {
                class: "floating-resize",
                "aria-hidden": "true",
                onpointerdown: move |evt| begin(evt, Gesture::Resize),
            }
        }
        if let Some(active) = drag() {
            div {
                class: "floating-drag-layer {gesture_class}",
                onpointermove: move |evt| {
                    let mut rect = rect;
                    let point = evt.client_coordinates();
                    rect.set(Some(active.apply((point.x, point.y), viewport())));
                },
                onpointerup: move |_| finish(),
                onpointercancel: move |_| finish(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f64, f64) = (1600.0, 900.0);

    #[test]
    fn centred_panel_shrinks_to_a_small_window() {
        let rect = PanelRect::centered((960.0, 720.0), (800.0, 600.0));
        assert_eq!(
            rect,
            PanelRect {
                x: 16.0,
                y: 16.0,
                w: 768.0,
                h: 568.0
            }
        );
    }

    #[test]
    fn a_position_saved_on_a_bigger_screen_comes_back_on_screen() {
        let saved = PanelRect {
            x: 2400.0,
            y: 1200.0,
            w: 900.0,
            h: 700.0,
        };
        let rect = saved.fit(VIEW);
        assert_eq!(rect.x, VIEW.0 - KEEP_VISIBLE);
        assert_eq!(rect.y, VIEW.1 - 40.0);
    }

    #[test]
    fn the_header_never_goes_above_the_top_edge() {
        let drag = Drag {
            gesture: Gesture::Move,
            start: PanelRect {
                x: 100.0,
                y: 50.0,
                w: 600.0,
                h: 400.0,
            },
            origin: (300.0, 60.0),
        };
        assert_eq!(drag.apply((300.0, -200.0), VIEW).y, 0.0);
    }

    #[test]
    fn resizing_stops_at_the_minimum_and_leaves_the_position_alone() {
        let drag = Drag {
            gesture: Gesture::Resize,
            start: PanelRect {
                x: 100.0,
                y: 50.0,
                w: 600.0,
                h: 400.0,
            },
            origin: (700.0, 450.0),
        };
        let rect = drag.apply((0.0, 0.0), VIEW);
        assert_eq!(
            rect,
            PanelRect {
                x: 100.0,
                y: 50.0,
                w: MIN_W,
                h: MIN_H
            }
        );
    }
}
