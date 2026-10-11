//! Whether proc-potential badges are drawn beside power names, kept across reloads like
//! [`crate::slot_level_store`] (the beta's `useUIStore.showProcPotential`).
//!
//! An analysis overlay, so it is opt-in: missing or corrupt state keeps it OFF, the beta's default.

use dioxus::prelude::*;

const KEY_SHOW_PROC_POTENTIAL: &str = "sk-show-proc-potential";

/// Persist the setting (fire-and-forget, like [`crate::level_up_store::persist`]).
pub fn persist(shown: bool) {
    let js = format!(
        "try {{ localStorage.setItem({KEY_SHOW_PROC_POTENTIAL:?}, {}); }} catch (_) {{}}",
        if shown { "\"true\"" } else { "\"false\"" }
    );
    crate::storage::commit(js);
}

/// The persisted setting; `None` (missing, or anything we didn't write) keeps the default.
pub async fn load() -> Option<bool> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_SHOW_PROC_POTENTIAL:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    match value.as_str()? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}
