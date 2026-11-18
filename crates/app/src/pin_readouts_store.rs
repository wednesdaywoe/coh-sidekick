//! Whether the desktop grid's top-row dashboards and Info stay on screen while the page scrolls
//! ([`crate::grid::view`]), kept across reloads like [`crate::proc_potential_store`].
//!
//! On by default — it is how the grid has behaved since the readouts were pinned — so missing or
//! corrupt state keeps it ON. Off, every surface scrolls with the page.

use dioxus::prelude::*;

const KEY_PIN_READOUTS: &str = "sk-pin-readouts";

/// The setting, provided at the shell root. A grid rendered outside the shell (a preview with no
/// provider) pins, the default.
#[derive(Clone, Copy)]
pub struct PinReadouts(pub Signal<bool>);

/// Persist the setting (fire-and-forget, like [`crate::proc_potential_store::persist`]).
pub fn persist(pinned: bool) {
    let js = format!(
        "try {{ localStorage.setItem({KEY_PIN_READOUTS:?}, {}); }} catch (_) {{}}",
        if pinned { "\"true\"" } else { "\"false\"" }
    );
    crate::storage::commit(js);
}

/// The persisted setting; `None` (missing, or anything we didn't write) keeps the default.
pub async fn load() -> Option<bool> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_PIN_READOUTS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    match value.as_str()? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}
