//! The page: drop or choose the game folder, then browse its bundles as folder trees and search
//! every file name in them at once.

use super::browser_files::{self, ObjectUrl, Picked};
use super::tree::{self, Dir};
use super::{
    default_folder, group_by_folder, kind_of, picture_name, size_label, text_of, thousands,
    zip_name, zip_refusal, Kind, ZipNames, ZIP_LIMIT,
};
use crate::build_file::{self, TextFileKind};
use coh_pigg::texture::{decode_texture, ImageKind};
use coh_pigg::zip::{DosTime, Zip};
use coh_pigg::{extract_entry, PiggEntry};
use dioxus::prelude::*;
use futures_util::StreamExt;
use std::collections::{BTreeSet, HashSet};

/// One `.pigg` found in the chosen folder. A table of contents that would not read stays in the
/// list with its reason, rather than the bundle quietly vanishing.
struct Bundle {
    name: String,
    size: f64,
    /// Sliced again for each file previewed or saved.
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
    /// Files ticked for a zip, by bundle and entry. Kept while the player moves between folders.
    ticked: BTreeSet<(usize, usize)>,
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
            }
            FolderHelp {}
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
                FolderHelp {}
            }
        }
        Stage::Failed(reason) => rsx! {
            p { class: "pigg-page__status pigg-page__status--error", role: "alert", "{reason}" }
            ChooseFolder { stage, again: true }
            FolderHelp {}
        },
        Stage::Ready(_) => rsx! { Browser { stage, shown, view } },
    }
}

/// Where each launcher puts the game, and what the browser will ask, before the player picks.
#[component]
fn FolderHelp() -> Element {
    let picker = browser_files::has_directory_picker();
    rsx! {
        section { class: "pigg-page__help",
            h2 { "Where is the game folder?" }
            ul {
                li {
                    strong { "Windows, Homecoming: " }
                    "usually "
                    code { "C:\\Games\\Homecoming" }
                    ". Open File Explorer, go to "
                    code { "C:\\Games" }
                    " and drag the Homecoming folder onto this page. If you installed it somewhere \
                     else, right-click your Homecoming shortcut and choose Open file location."
                }
                li {
                    strong { "Windows, Rebirth or Thunderspy (Sweet Tea launcher): " }
                    "press Windows+R, paste "
                    code { "%LOCALAPPDATA%\\Thunderspy Gaming" }
                    " and press Enter, then drag the Sweet Tea folder onto this page."
                }
                li {
                    strong { "Mac (LaunchCat): " }
                    "the game is inside your Library folder, which Finder hides. In Finder, press \
                     ⌘⇧G (Go to Folder), paste "
                    code { "~/Library/Application Support/LaunchCat" }
                    " and press Return, then drag the coh folder onto this page."
                }
                li {
                    strong { "Linux (Wine): " }
                    "usually "
                    code { "~/.wine/drive_c/Games/Homecoming" }
                    ". Folders whose names start with a dot are hidden; press Ctrl+H in your file \
                     manager to show them."
                }
            }
            p {
                "Dragging a folder one level too high or too low is fine: Pigg Wrangler looks \
                 inside it for the game's archives."
            }
            h2 { "NOTHING IS UPLOADED" }
            ul {
                li {
                    strong { "Pigg Wrangler is just tool for browsing and extracting files from game files that are already on your computer.\
                    Dragging a folder onto this page, or choosing a folder does not upload anything. The files are read in your browser and never leave your computer." }
                }
        }
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
        /// `1 file`, `40 files`.
        count: String,
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
        ticked: bool,
    },
}

/// The shown folder's bundles: the tree on the left, the picked folder's files or the search's
/// matches on the right.
#[component]
fn Browser(stage: Signal<Stage>, shown: Signal<usize>, view: Signal<View>) -> Element {
    // Bumped when a search match is opened, so the tree and list scroll to it once drawn.
    let reveal = use_signal(|| 0u32);
    let zipping = use_signal(|| Zipping::Idle);
    // Most players are here for the artwork, so a zip turns pictures into PNGs unless told not to.
    let as_png = use_signal(|| true);
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
        "{}, {}",
        counted(folder.bundles.len(), "archive"),
        counted(files, "file")
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
                        ticked: state.ticked.contains(&(bundle_index, entry)),
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
            heading = format!("{} in this folder and below", counted(dir.count, "file"));
            for child in &dir.dirs {
                list.push(ListLine::Folder {
                    bundle: *bundle_index,
                    path: child.path.clone(),
                    name: child.name.clone(),
                    count: counted(child.count, "file"),
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
                    ticked: state.ticked.contains(&(*bundle_index, entry)),
                });
            }
        }
    }
    let searching = !words.is_empty();
    let listed: Vec<(usize, usize)> = list
        .iter()
        .filter_map(|line| match line {
            ListLine::File { bundle, entry, .. } => Some((*bundle, *entry)),
            ListLine::Folder { .. } => None,
        })
        .collect();
    let all_ticked = !listed.is_empty() && listed.iter().all(|file| state.ticked.contains(file));
    let ticked_count = state.ticked.len();
    // The listed folder, if any: what "Save this folder as a zip" saves.
    let folder_zip = match (&state.folder, searching) {
        (Some((bundle, path)), false) => folder.bundles[*bundle]
            .tree
            .find(path)
            .map(|dir| (*bundle, path.clone(), dir.count)),
        _ => None,
    };
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
    let previewing = state.file.is_some();
    let query = state.query.clone();
    let search_hint = match folder.bundles.len() {
        1 => "in its 1 archive, e.g. powers.bin".to_string(),
        archives => format!("in all {archives} archives, e.g. powers.bin"),
    };
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
                placeholder: "{search_hint}",
                value: "{query}",
                oninput: move |event| view.write().query = event.value(),
            }
        }
        div {
            class: if previewing { "pigg-page__browser pigg-page__browser--preview" } else { "pigg-page__browser" },
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
                if searching || picked_folder || ticked_count > 0 {
                    ZipBar { stage, shown, view, zipping, as_png, folder_zip, ticked_count }
                }
                if searching || picked_folder {
                    if !list.is_empty() {
                        table { class: "pigg-page__files",
                            thead {
                                tr {
                                    th { class: "pigg-page__tick",
                                        input {
                                            r#type: "checkbox",
                                            aria_label: "Tick every file listed",
                                            title: "Tick every file listed",
                                            checked: all_ticked,
                                            disabled: listed.is_empty(),
                                            onchange: move |_| {
                                                let mut state = view.write();
                                                for file in &listed {
                                                    if all_ticked {
                                                        state.ticked.remove(file);
                                                    } else {
                                                        state.ticked.insert(*file);
                                                    }
                                                }
                                            },
                                        }
                                    }
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
                                                td { class: "pigg-page__tick" }
                                                td {
                                                    button {
                                                        class: "pigg-page__row-name pigg-page__row-name--folder",
                                                        r#type: "button",
                                                        onclick: move |_| show_folder(view, bundle, path.clone()),
                                                        "{name}/"
                                                    }
                                                }
                                                td { class: "pigg-page__number", "{count}" }
                                            }
                                        },
                                        ListLine::File { bundle, entry, label, parts, archive, size, chosen, ticked } => {
                                            let path = label.clone();
                                            rsx! {
                                                tr {
                                                    key: "f{bundle}/{entry}",
                                                    class: if chosen { "pigg-page__row--chosen" } else { "" },
                                                    td { class: "pigg-page__tick",
                                                        input {
                                                            r#type: "checkbox",
                                                            aria_label: "Tick {label}",
                                                            checked: ticked,
                                                            onchange: move |_| {
                                                                let mut state = view.write();
                                                                if !state.ticked.remove(&(bundle, entry)) {
                                                                    state.ticked.insert((bundle, entry));
                                                                }
                                                            },
                                                        }
                                                    }
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
            if previewing {
                PreviewPane { stage, shown, view }
            }
        }
    }
}

/// What the zip buttons are doing.
#[derive(Clone, PartialEq)]
enum Zipping {
    Idle,
    Working {
        done: usize,
        total: usize,
    },
    /// The saved zip's name and size, or why there is none.
    Done(Result<String, String>),
}

const ZIP_FILE: TextFileKind = TextFileKind {
    label: "Zip archive",
    extension: "zip",
    mime: "application/zip",
};

/// The zip buttons over the file list: the listed folder, or the ticked files, as one zip.
#[component]
fn ZipBar(
    stage: Signal<Stage>,
    shown: Signal<usize>,
    view: Signal<View>,
    zipping: Signal<Zipping>,
    as_png: Signal<bool>,
    /// The listed folder (bundle, path) and how many files are in it and below.
    folder_zip: Option<(usize, String, usize)>,
    ticked_count: usize,
) -> Element {
    let state = zipping();
    let working = matches!(state, Zipping::Working { .. });
    let mut view = view;
    let mut as_png = as_png;
    let folder_button = folder_zip.map(|(bundle, path, count)| {
        let label = if path.is_empty() {
            format!("Save this archive as a zip ({})", counted(count, "file"))
        } else {
            format!("Save this folder as a zip ({})", counted(count, "file"))
        };
        (bundle, path, label)
    });
    rsx! {
        div { class: "pigg-page__zip",
            div { class: "pigg-page__zip-row",
                if let Some((bundle, path, label)) = folder_button {
                    button {
                        class: "pigg-page__choose",
                        r#type: "button",
                        disabled: working,
                        onclick: move |_| {
                            let path = path.clone();
                            async move {
                                let Some((name, files)) = folder_files(stage, shown, bundle, &path) else { return };
                                save_zip(stage, shown, files, name, as_png(), zipping).await;
                            }
                        },
                        "{label}"
                    }
                }
                if ticked_count > 0 {
                    span { class: "pigg-page__zip-count", "{counted(ticked_count, \"file\")} ticked" }
                    button {
                        class: "pigg-page__choose",
                        r#type: "button",
                        disabled: working,
                        onclick: move |_| async move {
                            let files: Vec<(usize, usize)> = view.peek().ticked.iter().copied().collect();
                            save_zip(stage, shown, files, "pigg-wrangler-files.zip".to_string(), as_png(), zipping).await;
                        },
                        "Save the ticked files as a zip"
                    }
                    button {
                        class: "pigg-page__choose pigg-page__choose--quiet",
                        r#type: "button",
                        disabled: working,
                        onclick: move |_| view.write().ticked.clear(),
                        "Clear"
                    }
                }
            }
            label { class: "pigg-page__zip-option",
                input {
                    r#type: "checkbox",
                    checked: as_png(),
                    disabled: working,
                    onchange: move |event| as_png.set(event.checked()),
                }
                " Turn pictures (.texture) into PNGs in the zip. Zips can be up to 1 GB."
            }
            match state {
                Zipping::Idle => rsx! {},
                Zipping::Working { done, total } => rsx! {
                    p { class: "pigg-page__preview-note", role: "status",
                        "Zipping {thousands(done)} of {thousands(total)} files…"
                    }
                },
                Zipping::Done(Ok(saved)) => rsx! {
                    p { class: "pigg-page__preview-note", role: "status",
                        "Saved {saved}, wherever your browser puts downloads."
                    }
                },
                Zipping::Done(Err(reason)) => rsx! {
                    p { class: "pigg-page__status--error", role: "alert", "{reason}" }
                },
            }
        }
    }
}

/// A folder's zip name and every file in it and below.
fn folder_files(
    stage: Signal<Stage>,
    shown: Signal<usize>,
    bundle: usize,
    path: &str,
) -> Option<(String, Vec<(usize, usize)>)> {
    let staged = stage.peek();
    let Stage::Ready(folders) = &*staged else {
        return None;
    };
    let folder = folders.get((*shown.peek()).min(folders.len() - 1))?;
    let found = folder.bundles.get(bundle)?;
    let files = found.tree.find(path)?.all_files();
    let name = zip_name(&found.name, path);
    Some((
        name,
        files.into_iter().map(|entry| (bundle, entry)).collect(),
    ))
}

/// Zip these files and hand the zip to the browser, reporting progress in `zipping`.
async fn save_zip(
    stage: Signal<Stage>,
    shown: Signal<usize>,
    files: Vec<(usize, usize)>,
    name: String,
    as_png: bool,
    mut zipping: Signal<Zipping>,
) {
    if matches!(*zipping.peek(), Zipping::Working { .. }) {
        return;
    }
    // Everything the zip needs is gathered first, so moving around the page meanwhile is safe.
    let mut items = Vec::with_capacity(files.len());
    {
        let staged = stage.peek();
        let Stage::Ready(folders) = &*staged else {
            return;
        };
        let Some(folder) = folders.get((*shown.peek()).min(folders.len() - 1)) else {
            return;
        };
        for (bundle, entry) in files {
            let Some(found) = folder.bundles.get(bundle) else {
                continue;
            };
            let Some(entry) = found.index.as_ref().ok().and_then(|index| index.get(entry)) else {
                continue;
            };
            items.push((found.file.clone(), found.name.clone(), entry.clone()));
        }
    }
    let stored: u64 = items
        .iter()
        .map(|(_, _, entry)| entry.stored_range().end - entry.stored_range().start)
        .sum();
    if let Some(reason) = zip_refusal(items.len(), stored) {
        zipping.set(Zipping::Done(Err(reason)));
        return;
    }
    let total = items.len();
    zipping.set(Zipping::Working { done: 0, total });
    let outcome = match make_zip(items, stored, as_png, zipping).await {
        Ok(bytes) => {
            let size = size_label(bytes.len() as f64);
            build_file::write_bytes_file(&name, &bytes, &ZIP_FILE)
                .await
                .map(|_| format!("{name} ({size})"))
        }
        Err(reason) => Err(reason),
    };
    zipping.set(Zipping::Done(outcome.map_err(|reason| {
        if reason.starts_with("That ") || reason.starts_with("The zip") {
            reason
        } else {
            format!("The zip could not be made: {reason}")
        }
    })));
}

async fn make_zip(
    items: Vec<(web_sys::File, String, PiggEntry)>,
    stored: u64,
    as_png: bool,
    mut zipping: Signal<Zipping>,
) -> Result<Vec<u8>, String> {
    // A zip's dates are the player's clock time, not UTC.
    let now = js_sys::Date::new_0();
    let local = now.get_time() / 1000.0 - now.get_timezone_offset() * 60.0;
    // Room for the stored bytes and each file's two headers, so the buffer needn't regrow.
    let capacity = stored as usize + items.len() * 200;
    let mut zip = Zip::new(capacity, DosTime::from_unix(local.max(0.0) as u64));
    let mut names = ZipNames::default();
    let total = items.len();
    for (done, (file, bundle_name, entry)) in items.into_iter().enumerate() {
        let packed = browser_files::read_stored(&file, &entry).await?;
        let picture = (as_png && kind_of(&entry.path) == Kind::Picture)
            .then(|| {
                let bytes = extract_entry(&entry, &packed).ok()?;
                decode_texture(&bytes).ok()?.to_image_file().ok()
            })
            .flatten();
        match picture {
            // A picture that won't convert goes in as the game's own file, as in the Python tool.
            Some((image, kind)) => {
                let name = names.name(&bundle_name, &picture_name(&entry.path, kind.extension()));
                zip.add_file(&name, &image)
            }
            None => {
                let name = names.name(&bundle_name, &entry.path);
                zip.add_entry(&name, &entry, &packed).map(|_| ())
            }
        }
        .map_err(|error| error.to_string())?;
        // Pictures turned into PNGs can grow past what was checked up front.
        if zip.len() as u64 > ZIP_LIMIT {
            return Err(
                "The zip passed 1 GB, more than the browser can hold, so it was \
                        stopped. Choose a smaller folder, or leave pictures as game files."
                    .to_string(),
            );
        }
        if done % 25 == 24 {
            zipping.set(Zipping::Working {
                done: done + 1,
                total,
            });
        }
    }
    Ok(zip.finish())
}

/// What the preview pane shows for the picked file.
enum Preview {
    Picture {
        url: ObjectUrl,
        image: Vec<u8>,
        kind: ImageKind,
        size: Option<(u32, u32)>,
        format: &'static str,
        original: String,
    },
    Text {
        text: String,
        cut: bool,
    },
    Empty,
    Other,
    /// The file would not read out of its bundle.
    Failed(String),
    /// A picture the decoder will not draw.
    Refused(String),
}

const JPEG_FILE: TextFileKind = TextFileKind {
    label: "JPEG image",
    extension: "jpg",
    mime: "image/jpeg",
};

/// Small pictures are drawn bigger, by a whole number so each pixel stays square.
fn zoom_for(size: Option<(u32, u32)>) -> u32 {
    let longest = size.map_or(0, |(width, height)| width.max(height));
    if longest == 0 || longest >= 128 {
        1
    } else {
        (256 / longest).clamp(1, 4)
    }
}

async fn preview_of(file: web_sys::File, entry: &PiggEntry) -> Preview {
    // Decided before reading: powers.bin is 50 MB and has nothing to show.
    let kind = kind_of(entry.file_name());
    if entry.uncompressed_size == 0 {
        return Preview::Empty;
    }
    if kind == Kind::Other {
        return Preview::Other;
    }
    let bytes = match browser_files::read_entry(&file, entry).await {
        Ok(bytes) => bytes,
        Err(reason) => return Preview::Failed(reason),
    };
    match kind {
        Kind::Picture => {
            let texture = match decode_texture(&bytes) {
                Ok(texture) => texture,
                Err(reason) => return Preview::Refused(reason.to_string()),
            };
            let (image, kind) = match texture.to_image_file() {
                Ok(file) => file,
                Err(reason) => return Preview::Refused(reason.to_string()),
            };
            Preview::Picture {
                url: ObjectUrl::new(&image, kind.mime()),
                size: texture.size(),
                format: texture.format_name(),
                original: texture.original_name,
                image,
                kind,
            }
        }
        Kind::Text => {
            let (text, cut) = text_of(&bytes);
            Preview::Text { text, cut }
        }
        Kind::Other => Preview::Other,
    }
}

/// The picked file, shown: a picture with its size and format and a save button, or text.
#[component]
fn PreviewPane(stage: Signal<Stage>, shown: Signal<usize>, view: Signal<View>) -> Element {
    let picked = use_memo(move || view.read().file);
    // Which file the last save message is about, and how it went.
    let mut saved = use_signal(|| None::<((usize, usize), Result<String, String>)>);
    // Which file is being read to be saved as it is; powers.bin takes a moment.
    let mut saving = use_signal(|| None::<(usize, usize)>);
    let preview = use_resource(move || async move {
        let here = picked()?;
        let (file, entry) = picked_entry(stage, shown, here)?;
        let preview = preview_of(file, &entry).await;
        Some((here, entry, preview))
    });

    let current = picked();
    let read = preview.read();
    // While the next file reads, the last one's result is still held: show neither.
    let Some(Some((here, entry, shown_preview))) = &*read else {
        return rsx! {
            aside { class: "pigg-page__preview", p { class: "pigg-page__list-status", "Reading…" } }
        };
    };
    if Some(*here) != current {
        return rsx! {
            aside { class: "pigg-page__preview", p { class: "pigg-page__list-status", "Reading…" } }
        };
    }
    let here = *here;
    let name = entry.file_name().to_string();
    let size = size_label(f64::from(entry.uncompressed_size));
    let message = saved
        .read()
        .as_ref()
        .filter(|(about, _)| *about == here)
        .map(|(_, outcome)| outcome.clone());

    // The game's own file, byte for byte. Read again rather than kept: most files are never saved.
    let save_as_is = move |_| async move {
        let Some((file, entry)) = picked_entry(stage, shown, here) else {
            return;
        };
        saving.set(Some(here));
        let outcome = match browser_files::read_entry(&file, &entry).await {
            Ok(bytes) => {
                build_file::write_bytes_file(entry.file_name(), &bytes, &build_file::ANY_FILE).await
            }
            Err(reason) => Err(reason),
        };
        saving.set(None);
        saved.set(Some((here, outcome.map(|_| entry.file_name().to_string()))));
    };
    let busy = *saving.read() == Some(here);
    let as_is_label = if busy {
        "Saving…".to_string()
    } else {
        format!("Save {name}")
    };
    // A file that would not read out of its bundle would not save either.
    let can_save = !matches!(shown_preview, Preview::Failed(_));

    let mut facts = rsx! {};
    let mut picture_save = None;
    let body = match shown_preview {
        Preview::Picture {
            url,
            size: picture_size,
            format,
            original,
            kind,
            ..
        } => {
            let zoom = zoom_for(*picture_size);
            let line = match picture_size {
                Some((width, height)) => format!("{width} × {height} pixels · {format}"),
                None => format!("Size unknown · {format}"),
            };
            let style = match picture_size {
                Some((width, height)) if zoom > 1 => format!(
                    "width: {}px; height: {}px; image-rendering: pixelated;",
                    width * zoom,
                    height * zoom
                ),
                _ => String::new(),
            };
            picture_save = Some(match kind {
                ImageKind::Png => "Save as PNG",
                ImageKind::Jpeg => "Save as JPEG",
            });
            facts = rsx! {
                p { class: "pigg-page__preview-facts", "{line}" }
                if zoom > 1 {
                    p { class: "pigg-page__preview-note", "Shown at {zoom}× its real size." }
                }
            };
            let url = url.as_str().to_string();
            let original = original.clone();
            rsx! {
                div { class: "pigg-page__picture",
                    img { src: "{url}", alt: "{name}", style: "{style}" }
                }
                if !original.is_empty() {
                    p { class: "pigg-page__preview-note", "Made from {original}" }
                }
            }
        }
        Preview::Text { text, cut } => {
            let text = text.clone();
            let cut = *cut;
            rsx! {
                if cut {
                    p { class: "pigg-page__preview-note", "Only the first 100,000 characters are shown." }
                }
                pre { class: "pigg-page__text", "{text}" }
            }
        }
        Preview::Empty => rsx! {
            p { class: "pigg-page__list-status", "This file is empty." }
        },
        Preview::Other => rsx! {
            p { class: "pigg-page__list-status", "There is no preview for this kind of file." }
        },
        Preview::Failed(reason) => rsx! {
            p { class: "pigg-page__status--error", role: "alert", "This file could not be read: {reason}" }
        },
        Preview::Refused(reason) => rsx! {
            p { class: "pigg-page__status--error", role: "alert", "This picture can't be shown: {reason}" }
        },
    };

    rsx! {
        aside { class: "pigg-page__preview", aria_label: "Preview",
            div { class: "pigg-page__preview-head",
                h2 { class: "pigg-page__preview-name", "{name}" }
                button {
                    class: "pigg-page__preview-close",
                    r#type: "button",
                    aria_label: "Close the preview",
                    onclick: move |_| view.write().file = None,
                    "×"
                }
            }
            p { class: "pigg-page__preview-note", "{size}" }
            {facts}
            if can_save {
                div { class: "pigg-page__saves",
                    if let Some(label) = picture_save {
                        button {
                            class: "pigg-page__choose",
                            r#type: "button",
                            onclick: move |_| async move {
                                let file = {
                                    let read = preview.read();
                                    match &*read {
                                        Some(Some((about, entry, Preview::Picture { image, kind, .. }))) if *about == here => {
                                            Some((picture_name(entry.file_name(), kind.extension()), image.clone(), *kind))
                                        }
                                        _ => None,
                                    }
                                };
                                let Some((file_name, image, kind)) = file else { return };
                                let kind = match kind {
                                    ImageKind::Png => &build_file::PNG_FILE,
                                    ImageKind::Jpeg => &JPEG_FILE,
                                };
                                let outcome = build_file::write_bytes_file(&file_name, &image, kind)
                                    .await
                                    .map(|_| file_name);
                                saved.set(Some((here, outcome)));
                            },
                            "{label}"
                        }
                    }
                    button {
                        class: if picture_save.is_some() { "pigg-page__choose pigg-page__choose--quiet" } else { "pigg-page__choose" },
                        r#type: "button",
                        disabled: busy,
                        onclick: save_as_is,
                        "{as_is_label}"
                    }
                }
            }
            {body}
            match message {
                Some(Ok(file_name)) => rsx! {
                    p { class: "pigg-page__preview-note", role: "status", "Saved as {file_name}, wherever your browser puts downloads." }
                },
                Some(Err(reason)) => rsx! {
                    p { class: "pigg-page__status--error", role: "alert", "That could not be saved: {reason}" }
                },
                None => rsx! {},
            }
        }
    }
}

/// The bundle file and table-of-contents entry of a picked file, if it is still there.
fn picked_entry(
    stage: Signal<Stage>,
    shown: Signal<usize>,
    here: (usize, usize),
) -> Option<(web_sys::File, PiggEntry)> {
    let staged = stage.peek();
    let Stage::Ready(folders) = &*staged else {
        return None;
    };
    let folder = folders.get((*shown.peek()).min(folders.len() - 1))?;
    let bundle = folder.bundles.get(here.0)?;
    let entry = bundle.index.as_ref().ok()?.get(here.1)?.clone();
    Some((bundle.file.clone(), entry))
}

/// `1 file`, `6,696 files`.
fn counted(count: usize, thing: &str) -> String {
    match count {
        1 => format!("1 {thing}"),
        _ => format!("{} {thing}s", thousands(count)),
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
