//! The page: drop or choose the game folder, then browse its bundles as folder trees and search
//! every file name in them at once.

use super::browser_files::{self, Picked};
use super::tree::{self, Dir};
use super::{default_folder, group_by_folder, size_label, thousands};
use coh_pigg::PiggEntry;
use dioxus::prelude::*;
use futures_util::StreamExt;
use std::collections::HashSet;

/// One `.pigg` found in the chosen folder. A table of contents that would not read stays in the
/// list with its reason, rather than the bundle quietly vanishing.
struct Bundle {
    name: String,
    size: f64,
    /// Kept for the later reads: browsing and saving slice the same file.
    #[allow(dead_code)]
    file: web_sys::File,
    index: Result<Vec<PiggEntry>, String>,
    /// The folders inside, empty when the table of contents would not read.
    tree: Dir,
    /// Every entry path lowercased once, for searching as the player types.
    lower: Vec<String>,
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

/// What the player has opened and picked in the shown folder. Starts over with each new folder.
#[derive(Default)]
struct View {
    /// Folders opened in the tree, by bundle and path inside it (`""` is the bundle itself).
    open: HashSet<(usize, String)>,
    /// The folder whose files are listed.
    folder: Option<(usize, String)>,
    /// The file picked from a list or a search, by bundle and entry.
    file: Option<(usize, usize)>,
    query: String,
}

/// Matches past this many are counted but not listed: a word like `texture` matches thousands.
const SHOWN_MATCHES: usize = 500;

const FILE_INPUT_ID: &str = "pigg-file-input";
/// The page root, which takes the "something is being dragged over" highlight.
const PAGE_ID: &str = "pigg-page";

#[component]
pub fn Page() -> Element {
    let stage = use_signal(|| Stage::Start);
    let shown = use_signal(|| 0usize);
    let view = use_signal(View::default);
    // The reading runs here, not in the chooser that started it: the chooser leaves the page
    // while "Reading…" shows, and Dioxus cancels a task when its component goes.
    let loader = use_coroutine(move |mut picks: UnboundedReceiver<Picked>| async move {
        while let Some(picked) = picks.next().await {
            load(picked, stage, shown, view).await;
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
                div { class: "pigg-page__workspace", Workspace { stage, shown, view } }
            }
        }
    }
}

#[component]
fn Workspace(stage: Signal<Stage>, shown: Signal<usize>, view: Signal<View>) -> Element {
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
        Stage::Ready(_) => rsx! { Browser { stage, shown, view } },
    }
}

/// One line of the tree on the left, ready to draw: a bundle at depth 0, its folders below.
struct TreeLine {
    key: String,
    /// The bundle and folder a press opens.
    here: (usize, String),
    class: &'static str,
    indent: String,
    marker: &'static str,
    name: String,
    count: String,
    /// Why a bundle can't be opened: its table of contents would not read.
    unreadable: Option<String>,
    expanded: Option<bool>,
}

impl TreeLine {
    #[allow(clippy::too_many_arguments)]
    fn new(
        bundle: usize,
        path: String,
        depth: usize,
        name: String,
        count: Result<usize, String>,
        has_dirs: bool,
        open: bool,
        current: bool,
    ) -> Self {
        let (count, unreadable) = match count {
            Ok(count) => (thousands(count), None),
            Err(reason) => ("can't read".to_string(), Some(reason)),
        };
        TreeLine {
            key: format!("{bundle}/{path}"),
            here: (bundle, path),
            class: if current {
                "pigg-page__node pigg-page__node--current"
            } else {
                "pigg-page__node"
            },
            indent: format!("padding-left: {}px", 6 + depth * 16),
            marker: match (has_dirs, open) {
                (false, _) => "",
                (true, true) => "▾",
                (true, false) => "▸",
            },
            name,
            count,
            unreadable,
            expanded: has_dirs.then_some(open),
        }
    }
}

/// One line of the list on the right: a subfolder to step into, or a file.
enum ListLine {
    Folder {
        bundle: usize,
        path: String,
        name: String,
        count: usize,
    },
    File {
        bundle: usize,
        entry: usize,
        /// The name, or in search results the whole path.
        label: String,
        /// The label cut after each `/`, so a long path wraps between folders.
        parts: Vec<String>,
        archive: String,
        size: String,
        chosen: bool,
    },
}

/// The shown folder's bundles: the tree on the left, the picked folder's files or the search's
/// matches on the right.
#[component]
fn Browser(stage: Signal<Stage>, shown: Signal<usize>, view: Signal<View>) -> Element {
    // Bumped when a search match is opened, so the tree and list scroll to it once drawn.
    let reveal = use_signal(|| 0u32);
    use_effect(move || {
        if reveal() > 0 {
            document::eval(
                "for (const name of ['pigg-page__node--current', 'pigg-page__row--chosen']) \
                 document.querySelector('.' + name)?.scrollIntoView({ block: 'nearest' });",
            );
        }
    });

    let staged = stage.read();
    let Stage::Ready(folders) = &*staged else {
        return rsx! {};
    };
    let current = shown().min(folders.len() - 1);
    let folder = &folders[current];
    let state = view.read();

    let files: usize = folder.bundles.iter().map(|bundle| bundle.lower.len()).sum();
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

    let mut lines = Vec::new();
    for (bundle_index, bundle) in folder.bundles.iter().enumerate() {
        let open = state.open.contains(&(bundle_index, String::new()));
        let current = state.folder.as_ref() == Some(&(bundle_index, String::new()));
        lines.push(TreeLine::new(
            bundle_index,
            String::new(),
            0,
            bundle.name.clone(),
            bundle.index.as_ref().map(Vec::len).map_err(Clone::clone),
            !bundle.tree.dirs.is_empty(),
            open,
            current,
        ));
        if open {
            let is_open = |path: &str| state.open.contains(&(bundle_index, path.to_string()));
            let mut rows = Vec::new();
            bundle.tree.rows(1, &is_open, &mut rows);
            lines.extend(rows.into_iter().map(|row| {
                let current = state.folder.as_ref() == Some(&(bundle_index, row.path.clone()));
                TreeLine::new(
                    bundle_index,
                    row.path,
                    row.depth,
                    row.name,
                    Ok(row.count),
                    row.has_dirs,
                    row.open,
                    current,
                )
            }));
        }
    }

    let words = tree::search_words(&state.query);
    let mut heading = String::new();
    let mut crumbs: Vec<(usize, String, String)> = Vec::new();
    let mut list = Vec::new();
    if !words.is_empty() {
        let mut total = 0usize;
        for (bundle_index, bundle) in folder.bundles.iter().enumerate() {
            let Ok(entries) = &bundle.index else { continue };
            for (entry, lower) in bundle.lower.iter().enumerate() {
                if !tree::matches(&words, lower) {
                    continue;
                }
                total += 1;
                if list.len() < SHOWN_MATCHES {
                    list.push(ListLine::File {
                        bundle: bundle_index,
                        entry,
                        label: entries[entry].path.clone(),
                        parts: wrap_points(&entries[entry].path),
                        archive: bundle.name.clone(),
                        size: size_label(f64::from(entries[entry].uncompressed_size)),
                        chosen: false,
                    });
                }
            }
        }
        let query = state.query.trim();
        heading = match total {
            0 => format!("No file names here contain “{query}”."),
            1 => "1 file matches.".to_string(),
            _ if total > SHOWN_MATCHES => format!(
                "{} files match. The first {SHOWN_MATCHES} are listed; add a word to narrow it.",
                thousands(total)
            ),
            _ => format!("{} files match.", thousands(total)),
        };
    } else if let Some((bundle_index, path)) = &state.folder {
        let bundle = &folder.bundles[*bundle_index];
        if let (Ok(entries), Some(dir)) = (&bundle.index, bundle.tree.find(path)) {
            crumbs.push((*bundle_index, String::new(), bundle.name.clone()));
            for above in tree::folders_above(&format!("{path}/-")) {
                let name = above.rsplit('/').next().unwrap_or(&above).to_string();
                crumbs.push((*bundle_index, above, name));
            }
            heading = format!("{} in this folder and below", files_label(dir.count));
            for child in &dir.dirs {
                list.push(ListLine::Folder {
                    bundle: *bundle_index,
                    path: child.path.clone(),
                    name: child.name.clone(),
                    count: child.count,
                });
            }
            for &entry in &dir.files {
                list.push(ListLine::File {
                    bundle: *bundle_index,
                    entry,
                    label: entries[entry].file_name().to_string(),
                    parts: wrap_points(entries[entry].file_name()),
                    archive: String::new(),
                    size: size_label(f64::from(entries[entry].uncompressed_size)),
                    chosen: state.file == Some((*bundle_index, entry)),
                });
            }
        }
    }
    let searching = !words.is_empty();
    let overview: Vec<(usize, String, Result<String, String>, String)> = folder
        .bundles
        .iter()
        .enumerate()
        .map(|(index, bundle)| {
            let count = match &bundle.index {
                Ok(entries) => Ok(thousands(entries.len())),
                Err(reason) => Err(format!("Can't read: {reason}")),
            };
            (index, bundle.name.clone(), count, size_label(bundle.size))
        })
        .collect();
    let picked_folder = state.folder.is_some();
    let query = state.query.clone();
    let archives = folder.bundles.len();
    drop(state);
    drop(staged);

    let mut view = view;
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
                        onclick: move |_| {
                            shown.set(index);
                            view.set(View::default());
                        },
                        "{label}"
                    }
                }
            }
        }
        label { class: "pigg-page__search",
            span { class: "pigg-page__search-label", "Search file names" }
            input {
                class: "pigg-page__search-box",
                r#type: "search",
                placeholder: "in all {archives} archives, e.g. powers.bin",
                value: "{query}",
                oninput: move |event| view.write().query = event.value(),
            }
        }
        div { class: "pigg-page__browser",
            nav { class: "pigg-page__tree", aria_label: "Archives and their folders",
                for line in lines {
                    button {
                        key: "{line.key}",
                        class: line.class,
                        style: "{line.indent}",
                        r#type: "button",
                        aria_expanded: line.expanded,
                        disabled: line.unreadable.is_some(),
                        title: line.unreadable,
                        onclick: move |_| press_tree(view, line.here.clone()),
                        span { class: "pigg-page__node-marker", "{line.marker}" }
                        span { class: "pigg-page__node-name", "{line.name}" }
                        span { class: "pigg-page__node-count", "{line.count}" }
                    }
                }
            }
            section { class: "pigg-page__list",
                if !crumbs.is_empty() {
                    nav { class: "pigg-page__crumbs", aria_label: "Folder path",
                        for (bundle, path, name) in crumbs {
                            button {
                                key: "{path}",
                                class: "pigg-page__crumb",
                                r#type: "button",
                                onclick: move |_| show_folder(view, bundle, path.clone()),
                                "{name}"
                            }
                        }
                    }
                }
                if !heading.is_empty() {
                    p { class: "pigg-page__list-status", role: "status", "{heading}" }
                }
                if searching || picked_folder {
                    if !list.is_empty() {
                        table { class: "pigg-page__files",
                            thead {
                                tr {
                                    th { if searching { "File" } else { "Name" } }
                                    if searching {
                                        th { "Archive" }
                                    }
                                    th { class: "pigg-page__number", "Size" }
                                }
                            }
                            tbody {
                                for line in list {
                                    match line {
                                        ListLine::Folder { bundle, path, name, count } => rsx! {
                                            tr { key: "d{bundle}/{path}",
                                                td {
                                                    button {
                                                        class: "pigg-page__row-name pigg-page__row-name--folder",
                                                        r#type: "button",
                                                        onclick: move |_| show_folder(view, bundle, path.clone()),
                                                        "{name}/"
                                                    }
                                                }
                                                td { class: "pigg-page__number", "{files_label(count)}" }
                                            }
                                        },
                                        ListLine::File { bundle, entry, label, parts, archive, size, chosen } => {
                                            let path = label.clone();
                                            rsx! {
                                                tr {
                                                    key: "f{bundle}/{entry}",
                                                    class: if chosen { "pigg-page__row--chosen" } else { "" },
                                                    td {
                                                        button {
                                                            class: "pigg-page__row-name",
                                                            r#type: "button",
                                                            aria_pressed: chosen,
                                                            onclick: move |_| {
                                                                if searching {
                                                                    let folder = super::folder_of(&path).to_string();
                                                                    show_folder(view, bundle, folder);
                                                                    reveal_after(reveal);
                                                                }
                                                                view.write().file = Some((bundle, entry));
                                                            },
                                                            for part in parts {
                                                                "{part}"
                                                                wbr {}
                                                            }
                                                        }
                                                    }
                                                    if searching {
                                                        td { class: "pigg-page__row-archive", "{archive}" }
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
                } else {
                    p { class: "pigg-page__list-status",
                        "Open an archive on the left to see its folders, or search for a file by name."
                    }
                    table { class: "pigg-page__files",
                        thead {
                            tr {
                                th { "Archive" }
                                th { class: "pigg-page__number", "Files" }
                                th { class: "pigg-page__number", "Size" }
                            }
                        }
                        tbody {
                            for (index, name, count, size) in overview {
                                tr { key: "{name}",
                                    td {
                                        button {
                                            class: "pigg-page__row-name",
                                            r#type: "button",
                                            disabled: count.is_err(),
                                            onclick: move |_| show_folder(view, index, String::new()),
                                            "{name}"
                                        }
                                    }
                                    match &count {
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
}

/// `12 files`, `1 file`.
fn files_label(count: usize) -> String {
    match count {
        1 => "1 file".to_string(),
        _ => format!("{} files", thousands(count)),
    }
}

/// A path cut after each `/`, each piece keeping its slash.
fn wrap_points(path: &str) -> Vec<String> {
    path.split_inclusive('/').map(str::to_string).collect()
}

/// Asks the browser to scroll the opened folder and the picked file into view once they are drawn.
fn reveal_after(mut reveal: Signal<u32>) {
    reveal += 1;
}

/// Lists a folder's files and opens the tree down to it. Ends any search.
fn show_folder(mut view: Signal<View>, bundle: usize, path: String) {
    let mut state = view.write();
    state.open.insert((bundle, String::new()));
    for above in tree::folders_above(&format!("{path}/-")) {
        state.open.insert((bundle, above));
    }
    state.folder = Some((bundle, path));
    state.file = None;
    state.query.clear();
}

/// A press on a bundle or folder in the tree. The first opens it and lists its files; pressing
/// the listed one again closes or reopens it.
fn press_tree(mut view: Signal<View>, here: (usize, String)) {
    let listed = {
        let state = view.read();
        state.folder.as_ref() == Some(&here) && tree::search_words(&state.query).is_empty()
    };
    if !listed {
        show_folder(view, here.0, here.1);
        return;
    }
    let mut state = view.write();
    if !state.open.remove(&here) {
        state.open.insert(here);
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
async fn load(
    picked: Picked,
    mut stage: Signal<Stage>,
    mut shown: Signal<usize>,
    mut view: Signal<View>,
) {
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
        let entries = index.as_deref().unwrap_or_default();
        let tree = Dir::build(entries.iter().map(|entry| entry.path.as_str()));
        let lower = entries
            .iter()
            .map(|entry| entry.path.to_lowercase())
            .collect();
        bundles.push(Some(Bundle {
            name,
            size: file.size(),
            file,
            index,
            tree,
            lower,
        }));
    }
    let groups = group_by_folder(&paths);
    shown.set(default_folder(&groups));
    view.set(View::default());
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
