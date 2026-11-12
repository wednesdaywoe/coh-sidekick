//! Build bar — the sticky <900px bar that keeps the build's name, its level, undo, the switch
//! between the three build passes, and the four key numbers in reach while the stack scrolls.
//!
//! Nothing here computes what the app doesn't already compute: the level is the header's own
//! [`LevelControl`] (level is one build input with one writer), and the switch's counts read the
//! same budgets the Powers panel reads (see [`crate::phone_build::JobSwitch`]).

use crate::build_session::BuildSession;
use crate::level_control::{LevelControl, LevelUpControl};
use crate::mobile_nav::{OpenSheet, Sheet};
use crate::shell::{Db, UndoRedo};
use dioxus::prelude::*;

#[component]
pub fn BuildBar(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();

    // An empty build says what to do rather than "New build": on a phone this button is the
    // only way into the identity form, and a first visit has to see where a build starts.
    let (identity, unstarted) = {
        let build = session.build.read();
        match database.as_ref() {
            Some(db) if build.archetype.id.is_some() => {
                (crate::shell::identity_label(&build, db), false)
            }
            _ if build.name.is_empty() => ("Choose an archetype to start".to_string(), true),
            _ => (format!("{} · choose an archetype", build.name), true),
        }
    };
    let loaded = database.is_some();
    let mut sheet = use_context::<OpenSheet>().0;

    rsx! {
        div { class: "build-bar",
            // Its own row, the width of the bar: the build's name is what a reader checks first.
            button {
                class: if unstarted { "build-bar__identity is-unstarted" } else { "build-bar__identity" },
                r#type: "button",
                "aria-haspopup": "dialog",
                disabled: !loaded,
                onclick: move |_| sheet.set(Some(Sheet::Identity)),
                span { class: "build-bar__identity-label", "{identity}" }
                span { class: "popover-caret", "▼" }
            }
            // Level Up and Undo/Redo ride here because the header they live in is not drawn
            // below 900px: the bottom nav replaced it, and these three have no tab to go to.
            LevelControl { database: database.clone() }
            LevelUpControl { database: database.clone() }
            UndoRedo { session }
            crate::key_stats::KeyStatStrip {}
            crate::phone_build::JobSwitch { database }
        }
    }
}
