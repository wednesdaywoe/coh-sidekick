//! The page: choose the game folder, then see every bundle in it with its file count.

use super::browser_files::{self, Picked};
use super::{default_folder, group_by_folder, size_label, thousands};
use coh_pigg::PiggEntry;
use dioxus::prelude::*;
use futures_util::StreamExt;

/// One `.pigg` found in the chosen folder. A table of contents that would not read stays in the
/// list with its reason, rather than the bundle quietly vanishing.
struct Bundle {
    name: String,
    size: f64,
    /// Kept for the later reads: browsing and saving slice the same file.
    #[allow(dead_code)]
    file: web_sys::File,
    index: Result<Vec<PiggEntry>, String>,
}

/// A folder holding bundles, by its path from the chosen folder down.
struct Folder {
    path: String,
    bundles: Vec<Bundle>,
}

enum Stage {
    Start,
    Reading { done: usize, total: usize },
    Empty { root: String },
    Ready(Vec<Folder>),
    Failed(String),
}

const FOLDER_INPUT_ID: &str = "pigg-folder-input";

#[component]
pub fn Page() -> Element {
    let stage = use_signal(|| Stage::Start);
    let shown = use_signal(|| 0usize);
    // The reading runs here, not in the chooser that started it: the chooser leaves the page
    // while "Reading…" shows, and Dioxus cancels a task when its component goes.
    use_coroutine(move |mut picks: UnboundedReceiver<Picked>| async move {
        while let Some(picked) = picks.next().await {
            load(picked, stage, shown).await;
        }
    });

    rsx! {
        document::Title { "Pigg Wrangler · CoH Sidekick" }
        div { class: "pigg-page",
            header { class: "pigg-page__head",
                a { class: "pigg-page__back", href: "/", "← Back to the planner" }
                h1 { class: "pigg-page__name", "Pigg Wrangler" }
                p { class: "pigg-page__about",
                    "Browse data and save artwork and other assets from your CoH install."
                }
            }
            main { class: "pigg-page__body",
                // Shown on touch screens only (app.css): no phone or tablet can install the game.
                p { class: "pigg-page__touch",
                    "Pigg Wrangler reads the game's files from your computer, so it needs a \
                     Windows, Mac or Linux computer with City of Heroes installed."
                }
                div { class: "pigg-page__workspace", Workspace { stage, shown } }
            }
        }
    }
}

#[component]
fn Workspace(stage: Signal<Stage>, shown: Signal<usize>) -> Element {
    match &*stage.read() {
        Stage::Start => rsx! {
            p { class: "pigg-page__lead",
                "Choose the folder City of Heroes is installed in. Pigg Wrangler reads the \
                 game's archives (.pigg files) straight from your computer; nothing is uploaded."
            }
            ChooseFolder { stage, label: "Choose your game folder…" }
        },
        Stage::Reading { done, total } => rsx! {
            p { class: "pigg-page__status", role: "status",
                if *total == 0 {
                    "Looking for game archives…"
                } else {
                    "Reading archive {done + 1} of {total}…"
                }
            }
        },
        Stage::Empty { root } => {
            let place = if root.is_empty() {
                "that folder".to_string()
            } else {
                format!("“{root}”")
            };
            rsx! {
                p { class: "pigg-page__status", role: "status",
                    "There are no game archives (.pigg files) in {place} or any folder inside it. \
                     Choose the folder City of Heroes is installed in."
                }
                ChooseFolder { stage, label: "Choose a different folder…" }
            }
        }
        Stage::Failed(reason) => rsx! {
            p { class: "pigg-page__status pigg-page__status--error", role: "alert",
                "That folder could not be read: {reason}"
            }
            ChooseFolder { stage, label: "Choose a different folder…" }
        },
        Stage::Ready(folders) => {
            let current = (*shown.read()).min(folders.len() - 1);
            let folder = &folders[current];
            let files: usize = folder
                .bundles
                .iter()
                .filter_map(|bundle| bundle.index.as_ref().ok())
                .map(Vec::len)
                .sum();
            let unread = folder
                .bundles
                .iter()
                .filter(|bundle| bundle.index.is_err())
                .count();
            let summary = format!(
                "{} archives, {} files, in {}",
                thousands(folder.bundles.len()),
                thousands(files),
                folder.path
            );
            let choices: Vec<(usize, String, bool)> = folders
                .iter()
                .enumerate()
                .map(|(index, folder)| {
                    let label = format!("{} ({})", folder.path, folder.bundles.len());
                    (index, label, index == current)
                })
                .collect();
            let rows: Vec<(String, Result<String, String>, String)> = folder
                .bundles
                .iter()
                .map(|bundle| {
                    let count = match &bundle.index {
                        Ok(entries) => Ok(thousands(entries.len())),
                        Err(reason) => Err(format!("Can't read: {reason}")),
                    };
                    (bundle.name.clone(), count, size_label(bundle.size))
                })
                .collect();
            rsx! {
                div { class: "pigg-page__summary",
                    p { class: "pigg-page__status", role: "status",
                        "{summary}"
                        if unread > 0 {
                            span { class: "pigg-page__unread", " · {unread} could not be read" }
                        }
                    }
                    ChooseFolder { stage, label: "Choose a different folder…" }
                }
                if choices.len() > 1 {
                    div { class: "pigg-page__folders",
                        span { class: "pigg-page__folders-label",
                            "Game files are in {choices.len()} folders here:"
                        }
                        for (index, label, active) in choices {
                            button {
                                key: "{label}",
                                class: if active { "pigg-page__folder pigg-page__folder--shown" } else { "pigg-page__folder" },
                                r#type: "button",
                                aria_pressed: active,
                                onclick: move |_| shown.set(index),
                                "{label}"
                            }
                        }
                    }
                }
                table { class: "pigg-page__bundles",
                    thead {
                        tr {
                            th { "Archive" }
                            th { class: "pigg-page__number", "Files" }
                            th { class: "pigg-page__number", "Size" }
                        }
                    }
                    tbody {
                        for (name, count, size) in rows {
                            tr { key: "{name}",
                                td { "{name}" }
                                match count {
                                    Ok(count) => rsx! { td { class: "pigg-page__number", "{count}" } },
                                    Err(reason) => rsx! { td { class: "pigg-page__unread", "{reason}" } },
                                }
                                td { class: "pigg-page__number", "{size}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The folder chooser. Chrome and Edge get their own picker; other browsers get a folder input,
/// which has to be the thing the player clicks, so it sits inside the label.
#[component]
fn ChooseFolder(stage: Signal<Stage>, label: String) -> Element {
    let loader = use_coroutine_handle::<Picked>();
    if browser_files::has_directory_picker() {
        return rsx! {
            button {
                class: "pigg-page__choose",
                r#type: "button",
                onclick: move |_| async move {
                    match browser_files::pick_directory().await {
                        Ok(Some(picked)) => loader.send(picked),
                        Ok(None) => {}
                        Err(reason) => stage.set(Stage::Failed(reason)),
                    }
                },
                "{label}"
            }
        };
    }
    rsx! {
        label { class: "pigg-page__choose",
            "{label}"
            input {
                id: FOLDER_INPUT_ID,
                class: "pigg-page__folder-input",
                r#type: "file",
                directory: true,
                onchange: move |_| async move {
                    match browser_files::piggs_from_input(FOLDER_INPUT_ID) {
                        Ok(Some(picked)) => loader.send(picked),
                        Ok(None) => {}
                        Err(reason) => stage.set(Stage::Failed(reason)),
                    }
                },
            }
        }
    }
}

/// Reads every found bundle's table of contents, one after another, then groups them by folder.
async fn load(picked: Picked, mut stage: Signal<Stage>, mut shown: Signal<usize>) {
    if picked.found.is_empty() {
        stage.set(Stage::Empty { root: picked.root });
        return;
    }
    let total = picked.found.len();
    let paths: Vec<String> = picked.found.iter().map(|(path, _)| path.clone()).collect();
    let mut bundles = Vec::with_capacity(total);
    for (done, (path, file)) in picked.found.into_iter().enumerate() {
        stage.set(Stage::Reading { done, total });
        let index = browser_files::read_index(&file).await;
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        bundles.push(Some(Bundle {
            name,
            size: file.size(),
            file,
            index,
        }));
    }
    let groups = group_by_folder(&paths);
    shown.set(default_folder(&groups));
    let folders = groups
        .into_iter()
        .map(|(path, members)| Folder {
            path,
            bundles: members
                .into_iter()
                .filter_map(|index| bundles[index].take())
                .collect(),
        })
        .collect();
    stage.set(Stage::Ready(folders));
}
