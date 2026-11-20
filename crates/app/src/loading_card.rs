//! The second half of the loading sequence

use dioxus::prelude::*;

/// One of these shows under the slots, random each load.
const STATUS_LINES: &[&str] = &[
    "Freeeeming...",
    "Manipulating the AH...",
    "Jerk hacking...",
    "Rolling another alt...",
    "Waiting for the last player to click the button...",
    "Killing the anchor...",
    "Killing skuls...",
];

#[component]
pub fn LoadingCard() -> Element {
    // Picked once, so a redraw while loading doesn't change it.
    let status = use_hook(|| {
        let nanos = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize ^ d.as_millis() as usize)
            .unwrap_or(0);
        STATUS_LINES[nanos % STATUS_LINES.len()]
    });
    rsx! {
        div { class: "loading-card", role: "status", "aria-live": "polite",
            h2 { class: "loading-card__title", "Loading game data" }
            div { class: "loading-slots", "aria-hidden": "true",
                for i in 0..6 {
                    span { class: if i < 3 { "loading-slot is-on" } else { "loading-slot is-working" } }
                }
            }
            p { class: "loading-card__status", "{status}" }
            p { class: "loading-card__step", "Step 2 of 2" }
        }
    }
}
