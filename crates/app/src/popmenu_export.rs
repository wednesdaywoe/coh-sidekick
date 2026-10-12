//! The test-server popmenu: a `.mnu` file that grants every slotted enhancement in one click.
//!
//! What goes in the file is [`coh_data::popmenu`]'s; this module owns the surface — a menu
//! name, a save, and the list of pieces the menu will not grant. That list is the reason the
//! surface exists rather than the menu row saving straight away: the game answers a record it
//! does not know with a console line and grants nothing, so a piece left out has to be named
//! here or it arrives as an empty slot nobody can explain (POPMENU-1).

use crate::build_file::{self, SaveOutcome, TextFileKind};
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

/// Where the game reads popmenus from, inside the game folder.
const MENUS_FOLDER: &str = "data/texts/English/Menus";

/// Both the save window and the browser remember the folder under this name, so after the first
/// save into `Menus` the next one opens there.
const REMEMBER_AS: &str = "popmenu";

/// What the last save attempt has to say.
#[derive(Clone, PartialEq)]
enum Saved {
    Done(SaveOutcome, String),
    Failed(String),
}

/// Whether a saved file sits in a `texts/English/Menus` folder — the only place `/popmenu`
/// looks. Matched on the path's last three folders, ignoring case, so it holds for any install
/// location and either kind of slash.
fn in_menus_folder(path: &str) -> bool {
    let folders: Vec<String> = std::path::Path::new(path)
        .parent()
        .into_iter()
        .flat_map(|folder| folder.components().rev().take(3))
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    folders == ["menus", "english", "texts"]
}

#[component]
fn PopmenuExportBody(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();
    let mut name = use_signal(String::new);
    let mut saved = use_signal(|| Option::<Saved>::None);
    let mut warnings = use_signal(Vec::<PopmenuWarning>::new);
    // Which of the two copy buttons last copied, so it can say so.
    let mut copied = use_signal(|| Option::<&'static str>::None);

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
    let command = format!("/popmenu {shown_name}");

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
                    match build_file::save_text_file_remembering(
                        &file_name,
                        &menu.content,
                        &POPMENU_FILE,
                        REMEMBER_AS,
                    )
                    .await
                    {
                        Ok(SaveOutcome::Cancelled) => None,
                        Ok(outcome) => Some(Saved::Done(outcome, file_name)),
                        Err(reason) => Some(Saved::Failed(format!("Could not save: {reason}"))),
                    },
                );
            }
        }
    };

    let copy = move |which: &'static str, text: String| async move {
        if crate::clipboard::copy(&text).await.is_ok() {
            copied.set(Some(which));
        }
    };

    let left_out = warnings.read().len();
    rsx! {
        div { class: "build-report",
            p { class: "build-report__note",
                "A .mnu file that grants every enhancement in this build on the test server."
            }
            ol { class: "popmenu-export__steps",
                li {
                    "Save it into this folder, inside your game folder:"
                    div { class: "popmenu-export__copyrow",
                        span { class: "popmenu-export__path mono", "{MENUS_FOLDER}" }
                        button {
                            class: "seg",
                            r#type: "button",
                            onclick: move |_| copy("path", MENUS_FOLDER.to_string()),
                            if copied() == Some("path") { "Copied" } else { "Copy" }
                        }
                    }
                }
                li {
                    "In game, type:"
                    div { class: "popmenu-export__copyrow",
                        span { class: "popmenu-export__path mono", "{command}" }
                        button {
                            class: "seg",
                            r#type: "button",
                            onclick: {
                                let command = command.clone();
                                move |_| copy("command", command.clone())
                            },
                            if copied() == Some("command") { "Copied" } else { "Copy" }
                        }
                    }
                }
            }
            p { class: "build-report__note",
                "Enhancement boosters do not carry over, so boosted enhancements arrive at their \
                 base level. You can get free boosters from the Merit Vendor. Origin enhancements \
                 don't carry over either, but you can obtain them through the popmenu interface."
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
                    copied.set(None);
                },
            }
            div { class: "build-report__actions",
                button { class: "seg active", r#type: "button", onclick: save, "Save .mnu" }
            }
            match saved() {
                Some(Saved::Done(SaveOutcome::SavedTo(path), _)) if in_menus_folder(&path) => rsx! {
                    div { class: "build-report__line", "Saved to {path}" }
                },
                // The desktop knows where the file went, so it can catch the commonest mistake:
                // a menu saved anywhere but `Menus` is one the game never finds.
                Some(Saved::Done(SaveOutcome::SavedTo(path), _)) => rsx! {
                    div { class: "build-report__line", "Saved to {path}" }
                    div { class: "load-state error",
                        "That isn't a Menus folder, so the game won't find it. Save it again into "
                        span { class: "mono", "{MENUS_FOLDER}" }
                        " inside your game folder."
                    }
                },
                Some(Saved::Done(SaveOutcome::Picked, file_name)) => rsx! {
                    div { class: "build-report__line",
                        "Saved {file_name}. Next time, the save window opens in the same folder."
                    }
                },
                Some(Saved::Done(_, file_name)) => rsx! {
                    div { class: "build-report__line",
                        "Downloaded {file_name}. Move it from your downloads into the folder above. \
                         In Chrome or Edge it saves straight into the folder instead."
                    }
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

#[cfg(test)]
mod tests {
    use super::in_menus_folder;

    #[test]
    fn knows_a_menus_folder_wherever_the_game_is() {
        assert!(in_menus_folder(
            "/home/me/Homecoming/data/texts/English/Menus/Charnel.mnu"
        ));
        assert!(in_menus_folder(
            "/games/coh/DATA/TEXTS/english/menus/Charnel.mnu"
        ));
        assert!(!in_menus_folder("/home/me/Downloads/Charnel.mnu"));
        assert!(!in_menus_folder(
            "/home/me/Homecoming/data/texts/English/Charnel.mnu"
        ));
        assert!(!in_menus_folder("Charnel.mnu"));
    }
}
