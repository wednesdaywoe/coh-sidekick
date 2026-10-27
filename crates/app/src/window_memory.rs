//! The desktop app's main window reopens at the size, place and maximized state it was closed in.
//!
//! Kept in localStorage (`sk-window`) beside the popped-out panels' geometry
//! ([`crate::panel_popout`]), so it can only be read once the shell is up: the window opens at the
//! authored size and is moved to the saved one straight after. The size goes through the pop-outs'
//! converge loop, since on Linux `set_inner_size` and the DOM disagree by the decorations, and the
//! position is used only where the platform reports one ([`crate::panel_popout::WindowRect`]).

/// The main window's floor: app.css takes the phone layout below 900px, so the window may not
/// shrink to it. `main.rs` builds the window with this, and a restored size never goes under it.
#[cfg(feature = "desktop")]
pub const MIN_SIZE: (f64, f64) = (960.0, 640.0);

/// Restore the main window, then keep its record current. Call once, from the main window's root.
#[cfg(feature = "desktop")]
pub fn use_main_window_memory() {
    desktop::use_memory();
}

#[cfg(not(feature = "desktop"))]
pub fn use_main_window_memory() {}

#[cfg(feature = "desktop")]
mod desktop {
    use super::MIN_SIZE;
    use crate::panel_popout::{
        backend_reports_position, converge_inner_size, restorable_position, viewport_size,
        WindowRect,
    };
    use dioxus::desktop::tao::dpi::PhysicalPosition;
    use dioxus::desktop::tao::event::{Event as TaoEvent, WindowEvent};
    use dioxus::desktop::{use_wry_event_handler, LogicalPosition};
    use dioxus::prelude::*;
    use serde::{Deserialize, Serialize};

    const KEY: &str = "sk-window";

    /// The authored size, for a window first seen maximized: there is no normal size to keep yet.
    const DEFAULT_SIZE: (f64, f64) = (1440.0, 960.0);

    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    struct Saved {
        #[serde(flatten)]
        rect: WindowRect,
        /// The size and place underneath are the normal ones, kept for when it is unmaximized.
        #[serde(default)]
        maximized: bool,
    }

    pub fn use_memory() {
        // Nothing is recorded until the restore is done, or the opening Resized (at the authored
        // size) and the restore's own resizes would overwrite the record before it is read.
        let mut restored = use_signal(|| false);
        let mut saved = use_signal(|| None::<Saved>);
        use_hook(move || {
            spawn(async move {
                if let Some(record) = read().await {
                    restore(record).await;
                    saved.set(Some(record));
                }
                restored.set(true);
            })
        });

        let own_id = dioxus::desktop::window().id();
        use_wry_event_handler(move |event, _| {
            let TaoEvent::WindowEvent {
                window_id, event, ..
            } = event
            else {
                return;
            };
            if *window_id != own_id || !*restored.peek() {
                return;
            }
            match event {
                WindowEvent::Resized(_) => {
                    spawn(record_size(saved));
                }
                WindowEvent::Moved(position) => record_position(saved, *position),
                _ => {}
            }
        });
    }

    async fn read() -> Option<Saved> {
        let js =
            format!("try {{ return localStorage.getItem({KEY:?}); }} catch (_) {{ return null; }}");
        let value = document::eval(&js).await.ok()?;
        serde_json::from_str(value.as_str()?).ok()
    }

    fn write(record: Saved) {
        let Ok(json) = serde_json::to_string(&record) else {
            return;
        };
        crate::storage::commit(format!(
            "try {{ localStorage.setItem({KEY:?}, {json:?}); }} catch (_) {{}}"
        ));
    }

    async fn restore(record: Saved) {
        let window = dioxus::desktop::window();
        // A record from a larger screen, or a monitor since unplugged, must still fit this one.
        let (mut width, mut height) = (record.rect.width, record.rect.height);
        if let Some(monitor) = window.current_monitor() {
            let screen = monitor.size().to_logical::<f64>(monitor.scale_factor());
            width = width.min(screen.width);
            height = height.min(screen.height);
        }
        let (width, height) = (width.max(MIN_SIZE.0), height.max(MIN_SIZE.1));

        let position = restorable_position(Some(record.rect), backend_reports_position())
            .filter(|&(x, y)| on_a_monitor(x, y));
        if let Some((x, y)) = position {
            window.set_outer_position(LogicalPosition::new(x, y));
        }
        converge_inner_size(width, height).await;
        if record.maximized {
            window.set_maximized(true);
        }
    }

    /// Whether a window placed at `(x, y)` would have its title bar on some screen. A window
    /// restored onto a monitor that is no longer there opens where the OS puts it instead.
    fn on_a_monitor(x: f64, y: f64) -> bool {
        // A point a little inside the top-left corner, so a sliver at a screen's edge doesn't count.
        let (x, y) = (x + 40.0, y + 10.0);
        dioxus::desktop::window()
            .available_monitors()
            .any(|monitor| {
                let scale = monitor.scale_factor();
                let origin = monitor.position().to_logical::<f64>(scale);
                let size = monitor.size().to_logical::<f64>(scale);
                x >= origin.x
                    && x < origin.x + size.width
                    && y >= origin.y
                    && y < origin.y + size.height
            })
    }

    /// The content size from the DOM, as the pop-outs record theirs ([`viewport_size`] says why
    /// not tao's). While maximized only the flag changes, so the normal size survives under it.
    async fn record_size(mut saved: Signal<Option<Saved>>) {
        let window = dioxus::desktop::window();
        if window.is_minimized() {
            return;
        }
        let previous = *saved.peek();
        let record = if window.is_maximized() {
            let mut record = previous.unwrap_or(Saved {
                rect: WindowRect {
                    x: None,
                    y: None,
                    width: DEFAULT_SIZE.0,
                    height: DEFAULT_SIZE.1,
                },
                maximized: true,
            });
            record.maximized = true;
            record
        } else {
            let Some((width, height)) = viewport_size().await else {
                return;
            };
            // A window on its way to minimized can report a zero size first.
            if width < MIN_SIZE.0 / 2.0 || height < MIN_SIZE.1 / 2.0 {
                return;
            }
            Saved {
                rect: WindowRect {
                    x: previous.and_then(|record| record.rect.x),
                    y: previous.and_then(|record| record.rect.y),
                    width,
                    height,
                },
                maximized: false,
            }
        };
        if previous != Some(record) {
            saved.set(Some(record));
            write(record);
        }
    }

    /// The position from a `Moved` event's payload, as a pop-out's. Ignored while maximized, so
    /// unmaximizing goes back to where the window was.
    fn record_position(mut saved: Signal<Option<Saved>>, moved_to: PhysicalPosition<i32>) {
        let window = dioxus::desktop::window();
        if !backend_reports_position() || window.is_maximized() || window.is_minimized() {
            return;
        }
        let Some(mut record) = *saved.peek() else {
            return;
        };
        let moved_to = moved_to.to_logical::<f64>(window.scale_factor());
        record.rect.x = Some(moved_to.x);
        record.rect.y = Some(moved_to.y);
        saved.set(Some(record));
        write(record);
    }
}
