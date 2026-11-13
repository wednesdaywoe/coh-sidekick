//! "Save to file…": the build as a Sidekick `.skif`, or as a Mids Reborn `.mbd`.
//!
//! A window rather than a straight save because the Mids choice has things to say first: a
//! server Mids has no database for, chance procs Mids won't count, and anything the writer
//! couldn't carry over ([`coh_data::mbd_export`]). They're shown before the save, not after.

use crate::build_file::{self, TextFileKind};
use crate::build_session::BuildSession;
use crate::modal::{Modal, ModalSize};
use crate::panels::stats::BuildTotals;
use crate::picker_defaults::PickerDefaults;
use crate::shell::Db;
use coh_data::level::Level;
use coh_data::mbd_export::{self, ExportNote};
use coh_data::skif;
use coh_math::procs::ProcSourceKind;
use dioxus::prelude::*;
use std::collections::BTreeSet;

const MIDS_FILE: TextFileKind = TextFileKind {
    label: "Mids Reborn build",
    extension: "mbd",
    mime: "application/json",
};

/// The window's open flag, provided by the shell.
#[derive(Clone, Copy)]
pub struct SaveFileOpen(pub Signal<bool>);

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Sidekick,
    Mids,
}

/// Mounted by the shell above both layout roots. Renders nothing while closed.
#[component]
pub fn SaveFileHost(database: Option<Db>) -> Element {
    let mut open = use_context::<SaveFileOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Save to file".to_string(),
            size: ModalSize::Md,
            on_close: move |_| open.set(false),
            SaveFileBody { database }
        }
    }
}

/// What the Mids file will be, worked out before the save so its warnings come first.
#[derive(Clone, PartialEq)]
struct MidsPreview {
    text: Result<String, String>,
    server: Option<String>,
    chance_procs: Vec<String>,
    notes: Vec<ExportNote>,
}

#[component]
fn SaveFileBody(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();
    let totals = use_context::<BuildTotals>().0;
    let io_level = use_context::<PickerDefaults>().io_level;
    let mut open = use_context::<SaveFileOpen>().0;
    let mut format = use_signal(|| Format::Sidekick);
    let mut failed = use_signal(|| Option::<String>::None);

    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };

    let mids = use_memo({
        let database = database.clone();
        move || {
            if format() != Format::Mids {
                return None;
            }
            let build = session.build.read();
            let level = Level::new(io_level()).unwrap_or(Level::constant(50));
            let written = mbd_export::write_mbd(&build, &database, level);
            // Mids leaves chance procs out of its totals; Sidekick counts their average.
            let chance_procs: BTreeSet<String> = totals
                .read()
                .proc_breakdown
                .iter()
                .filter(|source| source.kind != ProcSourceKind::AlwaysOn)
                .map(|source| match source.set_name.as_str() {
                    "" => source.proc_name.clone(),
                    set => format!("{set}: {}", source.proc_name),
                })
                .collect();
            Some(MidsPreview {
                server: mbd_export::database_warning(build.dataset),
                chance_procs: chance_procs.into_iter().collect(),
                notes: written
                    .as_ref()
                    .map(|export| export.notes.clone())
                    .unwrap_or_default(),
                text: written
                    .map(|export| export.text())
                    .map_err(|error| error.to_string()),
            })
        }
    });

    let save = move |_| {
        let database = database.clone();
        async move {
            // Everything read before the await: a read guard held across one outlives its render.
            let stem = build_file::file_stem_for(&session.build.read(), &database);
            let (file_name, text, kind) = match format() {
                Format::Sidekick => (
                    format!("{stem}.{}", build_file::EXTENSION),
                    skif::encode(&session.build.read(), &database).map_err(|e| e.to_string()),
                    &build_file::BUILD_FILE,
                ),
                Format::Mids => (
                    format!("{stem}.{}", MIDS_FILE.extension),
                    mids()
                        .map(|preview| preview.text)
                        .unwrap_or_else(|| Err("nothing to write".to_string())),
                    &MIDS_FILE,
                ),
            };
            let written = match text {
                Ok(text) => build_file::write_text_file(&file_name, &text, kind).await,
                Err(reason) => Err(reason),
            };
            match written {
                // The download or the save dialog is its own receipt.
                Ok(_) => open.set(false),
                Err(reason) => failed.set(Some(format!("Couldn't save {file_name}: {reason}"))),
            }
        }
    };

    let preview = mids();
    let can_save = preview.as_ref().is_none_or(|preview| preview.text.is_ok());
    let note_count = preview.as_ref().map_or(0, |preview| preview.notes.len());

    rsx! {
        div { class: "build-report save-file",
            div { class: "build-report__actions", role: "group", "aria-label": "File format",
                for (option , label) in [
                    (Format::Sidekick, "Sidekick (.skif)"),
                    (Format::Mids, "Mids Reborn (.mbd)"),
                ]
                {
                    button {
                        key: "{label}",
                        class: if format() == option { "seg active" } else { "seg" },
                        r#type: "button",
                        "aria-pressed": format() == option,
                        onclick: move |_| {
                            format.set(option);
                            failed.set(None);
                        },
                        "{label}"
                    }
                }
            }
            match format() {
                Format::Sidekick => rsx! {
                    p { class: "build-report__note",
                        "The whole build, in this planner's own format. Open it again here, or \
                         send it to someone who uses Sidekick."
                    }
                },
                Format::Mids => rsx! {
                    p { class: "build-report__note", "A build file Mids Reborn can open." }
                },
            }
            if let Some(preview) = &preview {
                if let Some(server) = &preview.server {
                    div { class: "save-file__warning", "{server}" }
                }
                if let Err(reason) = &preview.text {
                    div { class: "load-state error", "Can't write a Mids file: {reason}" }
                }
                if !preview.chance_procs.is_empty() {
                    div { class: "save-file__warning",
                        "Mids doesn't count chance-based procs in its totals, so its numbers \
                         will be lower than Sidekick's. This build has: "
                        "{preview.chance_procs.join(\", \")}."
                    }
                }
                if note_count > 0 {
                    p { class: "build-report__line",
                        if note_count == 1 {
                            "1 thing Mids will show differently"
                        } else {
                            "{note_count} things Mids will show differently"
                        }
                    }
                    ul { class: "build-report__unresolved",
                        for (i , note) in preview.notes.iter().enumerate() {
                            li { key: "{i}",
                                span { class: "build-report__context", "{note.context}" }
                                span { class: "build-report__detail", "{note.detail}" }
                            }
                        }
                    }
                }
            }
            if let Some(reason) = failed() {
                div { class: "load-state error", "{reason}" }
            }
            div { class: "build-report__actions",
                button {
                    class: "seg active",
                    r#type: "button",
                    disabled: !can_save,
                    onclick: save,
                    match format() {
                        Format::Sidekick => "Save .skif",
                        Format::Mids => "Save .mbd",
                    }
                }
            }
        }
    }
}
