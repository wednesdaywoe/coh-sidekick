//! The page: drop or choose the game folder, then see every bundle in it with its file count.

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

const FILE_INPUT_ID: &str = "pigg-file-input";
/// The page root, which takes the "something is being dragged over" highlight.
const PAGE_ID: &str = "pigg-page";

#[component]
pub fn Page() -> Element {
    let stage = use_signal(|| Stage::Start);
    let shown = use_signal(|| 0usize);
    // The reading runs here, not in the chooser that started it: the chooser leaves the page
    // while "Reading…" shows, and Dioxus cancels a task when its component goes.
    let loader = use_coroutine(move |mut picks: UnboundedReceiver<Picked>| async move {
        while let Some(picked) = picks.next().await {
            load(picked, stage, shown).await;
        }
    });
    // A folder dropped anywhere on the page, at any stage, is read like a chosen one.
    use_future(move || {
        let mut stage = stage;
        async move {
            loop {
                match browser_files::next_drop(PAGE_ID).await {
                    Ok(Some(picked)) => loader.send(picked),
                    Ok(None) => {}
                    Err(reason) => stage.set(Stage::Failed(reason)),
                }
            }
        }
    });

    rsx! {
        document::Title { "Pigg Wrangler · CoH Sidekick" }
        div { class: "pigg-page", id: PAGE_ID,
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
                "Pigg Wrangler reads the game's archives (.pigg files) straight from your \
                 computer. Nothing is uploaded."
            }
            div { class: "pigg-page__drop",
                p { class: "pigg-page__drop-title",
                    "Drag the folder City of Heroes is installed in onto this page"
                }
                p { class: "pigg-page__drop-or", "or" }
                ChooseFolder { stage, again: false }
                if !browser_files::has_directory_picker() {
                    p { class: "pigg-page__drop-hint",
                        "Open the live folder inside the game's assets folder, select every \
                         file (Ctrl+A, or ⌘A on a Mac), then click Open."
                    }
                }
            }
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
            // No root means loose files were chosen or dropped, not a folder.
            let missing = if root.is_empty() {
                "None of those are game archives (.pigg files).".to_string()
            } else {
                format!(
                    "There are no game archives (.pigg files) in “{root}” or any folder inside it."
                )
            };
            rsx! {
                p { class: "pigg-page__status", role: "status",
                    "{missing} Drag the folder City of Heroes is installed in onto this page, or \
                     choose again."
                }
                ChooseFolder { stage, again: true }
            }
        }
        Stage::Failed(reason) => rsx! {
            p { class: "pigg-page__status pigg-page__status--error", role: "alert",
                "That could not be read: {reason}"
            }
            ChooseFolder { stage, again: true }
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
            let mut summary = format!(
                "{} archives, {} files",
                thousands(folder.bundles.len()),
                thousands(files)
            );
            // Loose files chosen through the file input have no folder to name.
            if !folder.path.is_empty() {
                summary.push_str(&format!(", in {}", folder.path));
            }
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
                    ChooseFolder { stage, again: true }
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

/// The chooser button. Chrome and Edge get their own folder picker; other browsers choose the
/// `.pigg` files themselves, through a file input that has to be the thing the player clicks, so
/// it sits inside the label. `again` words it for a second choice.
#[component]
fn ChooseFolder(stage: Signal<Stage>, again: bool) -> Element {
    let loader = use_coroutine_handle::<Picked>();
    if browser_files::has_directory_picker() {
        let label = if again {
            "Choose a different folder…"
        } else {
            "Choose your game folder…"
        };
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
    let label = if again {
        "Choose different .pigg files…"
    } else {
        "Choose the .pigg files…"
    };
    rsx! {
        label { class: "pigg-page__choose",
            "{label}"
            input {
                id: FILE_INPUT_ID,
                class: "pigg-page__file-input",
                r#type: "file",
                multiple: true,
                onchange: move |_| async move {
                    match browser_files::piggs_from_input(FILE_INPUT_ID) {
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
