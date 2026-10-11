//! The test-server popmenu: a `.mnu` file that grants every slotted enhancement in one click.
//!
//! What goes in the file is [`coh_data::popmenu`]'s; this module owns the surface — a menu
//! name, a save, and the list of pieces the menu will not grant. That list is the reason the
//! surface exists rather than the menu row saving straight away: the game answers a record it
//! does not know with a console line and grants nothing, so a piece left out has to be named
//! here or it arrives as an empty slot nobody can explain (POPMENU-1).

use crate::build_file::{self, TextFileKind};
use crate::build_session::BuildSession;
use crate::modal::{Modal, ModalSize};
use crate::shell::Db;
use coh_data::popmenu::{self, PopmenuWarning};
use dioxus::prelude::*;

const POPMENU_FILE: TextFileKind = TextFileKind {
    label: "City of Heroes popmenu",
    extension: "mnu",
    mime: "text/plain",
};

/// The modal's open flag, provided by the shell.
#[derive(Clone, Copy)]
pub struct PopmenuExportOpen(pub Signal<bool>);

/// Mounted by the shell above both layout roots. Renders nothing while closed.
#[component]
pub fn PopmenuExportHost(database: Option<Db>) -> Element {
    let mut open = use_context::<PopmenuExportOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Test server popmenu".to_string(),
            size: ModalSize::Md,
            on_close: move |_| open.set(false),
            PopmenuExportBody { database }
        }
    }
}

/// What the last save attempt has to say.
#[derive(Clone, PartialEq)]
enum Saved {
    Done(String),
    Failed(String),
}

#[component]
fn PopmenuExportBody(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();
    let mut name = use_signal(String::new);
    let mut saved = use_signal(|| Option::<Saved>::None);
    let mut warnings = use_signal(Vec::<PopmenuWarning>::new);

    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };
    let default_name = popmenu::default_menu_name(&session.build.read());
    let shown_name = if name.read().is_empty() {
        default_name.clone()
    } else {
        name()
    };

    let save = {
        let shown_name = shown_name.clone();
        move |_| {
            let database = database.clone();
            let menu_name = shown_name.clone();
            async move {
                let Some(index) = database.boost_index.as_ref() else {
                    saved.set(Some(Saved::Failed(
                        "This dataset carries no boost index, so no enhancement can be named"
                            .into(),
                    )));
                    return;
                };
                let stamp = stamp_now();
                // Derived before the await: a `Signal` read guard held across one would outlive
                // the render it belongs to.
                let menu =
                    popmenu::generate(&session.build.read(), &database, index, &menu_name, &stamp);
                warnings.set(menu.warnings);
                let file_name = format!("{menu_name}.mnu");
                saved.set(
                    match build_file::write_text_file(&file_name, &menu.content, &POPMENU_FILE)
                        .await
                    {
                        Ok(_) => Some(Saved::Done(format!("Saved {file_name}"))),
                        Err(reason) => Some(Saved::Failed(format!("Could not save: {reason}"))),
                    },
                );
            }
        }
    };

    let left_out = warnings.read().len();
    rsx! {
        div { class: "build-report",
            p { class: "build-report__note",
                "A .mnu file that grants every enhancement in this build on the test server. Put it in "
                span { class: "mono", "data/texts/English/Menus/" }
                " in your game folder, then type "
                span { class: "mono", "/popmenu {shown_name}" }
                " in game."
            }
            // The game's `boost` command takes no combine count, so no popmenu can carry
            // boosters. Said here because the alternative is a bug report.
            p { class: "build-report__note",
                "Enhancement boosters are not carried: the game's grant command cannot state them, \
                 so boosted enhancements arrive at their base level. Origin enhancements (TO/DO/SO) \
                 are left out too — buy those in game."
            }
            label { class: "field-label", "Menu name" }
            input {
                class: "sb-save__input",
                value: "{name}",
                maxlength: 50,
                placeholder: "{default_name}",
                oninput: move |e| {
                    // Letters, digits and `_` only: the name is typed after `/popmenu` and is
                    // the file's stem, and has to survive both.
                    name.set(
                        e.value()
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                            .collect(),
                    );
                },
            }
            div { class: "build-report__actions",
                button { class: "seg active", r#type: "button", onclick: save, "Save .mnu" }
            }
            match saved() {
                Some(Saved::Done(line)) => rsx! {
                    div { class: "build-report__line", "{line}" }
                },
                Some(Saved::Failed(reason)) => rsx! {
                    div { class: "load-state error", "{reason}" }
                },
                None => rsx! {},
            }
            if left_out > 0 {
                p { class: "build-report__line",
                    if left_out == 1 {
                        "1 enhancement the menu will not grant"
                    } else {
                        "{left_out} enhancements the menu will not grant"
                    }
                }
                ul { class: "build-report__unresolved",
                    for (i , warning) in warnings.read().iter().enumerate() {
                        li { key: "{i}",
                            span { class: "build-report__context",
                                "{warning.power} slot {warning.slot}"
                            }
                            span { class: "build-report__detail", "{warning.detail}" }
                        }
                    }
                }
            }
        }
    }
}

/// The header's generation time, in UTC. Read through `web_time` because `chrono::Local::now()`
/// panics on `wasm32-unknown-unknown`, which has no clock of its own — and a local offset is a
/// second thing the web build cannot read.
fn stamp_now() -> String {
    let at_ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|at| at.format("%m/%d/%Y %H:%M:%S UTC").to_string())
        .unwrap_or_default()
}
