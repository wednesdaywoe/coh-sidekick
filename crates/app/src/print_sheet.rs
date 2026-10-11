//! Print sheet — the build on paper.
//!
//! The beta's `export-print.ts`, which the port missed: a header, the dashboard's numbers, and
//! every power grouped by powerset with what is slotted in it. The beta wrote a standalone HTML
//! page into a new tab and let that tab call `window.print()`. Neither half survives here: the
//! desktop build has no second tab to open, and WKWebView (the macOS webview) ignores a page's
//! own `window.print()` outright.
//!
//! # How a page gets printed
//!
//! The sheet is ordinary DOM, rendered in the modal as its own preview. Printing copies that
//! preview into `#sidekick-print`, a direct child of `<body>`, and the print stylesheet in
//! `app.css` hides every other child of `<body>` — so the app's fixed-height, overflow-clipped
//! shell never reaches the printer and the sheet paginates like a document. The copy, not the
//! modal itself, is what prints, because the modal sits inside that shell and inherits its
//! clipping.
//!
//! What starts the print differs by target, and only that differs: the web build calls
//! `window.print()`, the desktop build asks the webview through `wry` ([`print_page`]), which has
//! a native print operation on all three desktop platforms.
//!
//! # Two layouts
//!
//! **Detailed** is the beta's: one line per slot, its level, and the piece by set and aspects.
//! **Compact** is what Mids prints and what a player asked for by name: one line per power, each
//! slot as the aspects it enhances and the level it was granted at — `Acc/Dmg(1), Dmg(3)`.
//!
//! Slot levels are [`coh_data::slot_levels`]' answer, as in the forum post, and a slot that
//! answer cannot place prints `?` rather than its power's pick level (the beta's SLOT-1 lesson).
//! Unlike the post, the base slot prints its level rather than `A`: on paper the reader wants
//! the number, and the base slot's level is the pick's own.

use crate::build_session::BuildSession;
use crate::modal::{Modal, ModalSize};
use crate::naming;
use crate::panels::dashboards::{DashboardConfig, Dashboards};
use crate::panels::stat_registry::{self, StatSection};
use crate::panels::stats::BuildTotals;
use crate::shell::Db;
use coh_data::{CharacterState, Enhancement, EnhancementKind, SelectedPower};
use coh_math::CalculatedTotals;
use dioxus::prelude::*;

/// How the powers are laid out. Everything else on the sheet is the same in both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrintLayout {
    Detailed,
    Compact,
}

impl PrintLayout {
    pub const ALL: [PrintLayout; 2] = [PrintLayout::Detailed, PrintLayout::Compact];

    fn label(self) -> &'static str {
        match self {
            PrintLayout::Detailed => "Detailed",
            PrintLayout::Compact => "Compact",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            PrintLayout::Detailed => "One line per slot, with each piece's set and level",
            PrintLayout::Compact => "One line per power, each slot as its type and level",
        }
    }

    fn id(self) -> &'static str {
        match self {
            PrintLayout::Detailed => "detailed",
            PrintLayout::Compact => "compact",
        }
    }
}

// ============================================================
// The sheet, as data.
// ============================================================

/// Everything the sheet reads, gathered once so the builders below stay pure over it.
pub struct SheetInput<'a> {
    pub build: &'a CharacterState,
    pub database: &'a Db,
    pub totals: &'a CalculatedTotals,
    /// The dashboard's visible rows — the sheet prints what the dashboard shows, as the forum
    /// post does, rather than a second curated list.
    pub visibility: &'a Dashboards,
    pub slot_levels: Option<&'a coh_data::slot_levels::SlotLevels>,
}

#[derive(Clone, PartialEq, Debug)]
struct Sheet {
    title: String,
    subtitle: String,
    source: String,
    stats: Vec<StatGroup>,
    groups: Vec<PowerGroup>,
    incarnates: Vec<(String, String)>,
}

#[derive(Clone, PartialEq, Debug)]
struct StatGroup {
    title: String,
    rows: Vec<(String, String)>,
}

#[derive(Clone, PartialEq, Debug)]
struct PowerGroup {
    title: String,
    powers: Vec<SheetPower>,
}

#[derive(Clone, PartialEq, Debug)]
struct SheetPower {
    /// `0` is a power the game granted rather than one picked, and prints no level.
    level: u8,
    name: String,
    slots: Vec<SheetSlot>,
    empty_slots: usize,
}

#[derive(Clone, PartialEq, Debug)]
struct SheetSlot {
    /// `None` where the schedule has no grant for it — printed `?`.
    level: Option<u8>,
    /// What the piece enhances, abbreviated: `Acc/Dmg`. The whole of a compact slot.
    short: String,
    /// The piece in full, for the detailed layout: `50 Thunderstrike - Acc/Dmg`.
    long: String,
}

impl SheetSlot {
    fn level_text(&self) -> String {
        self.level
            .map_or_else(|| "?".to_string(), |level| level.to_string())
    }
}

fn build_sheet(input: &SheetInput) -> Sheet {
    let build = input.build;
    let database = input.database;

    let title = match build.name.trim() {
        "" => "Unnamed build".to_string(),
        name => name.to_string(),
    };

    let mut subtitle = match naming::archetype_name(build, database) {
        Some(archetype) => format!("Level {} {archetype}", build.level),
        None => format!("Level {}", build.level),
    };
    if let Some(origin) = build.origin.as_deref().filter(|o| !o.is_empty()) {
        subtitle.push_str(&format!(" — {origin}"));
    }
    let primary = naming::powerset_name(&build.primary, database);
    let secondary = naming::powerset_name(&build.secondary, database);
    match (primary.as_deref(), secondary.as_deref()) {
        (Some(p), Some(s)) => subtitle.push_str(&format!(" — {p} / {s}")),
        (Some(one), None) | (None, Some(one)) => subtitle.push_str(&format!(" — {one}")),
        (None, None) => {}
    }

    let mut groups = Vec::new();
    let mut group = |title: Option<String>, powers: &[SelectedPower], slotted_only: bool| {
        let powers: Vec<SheetPower> = sorted_by_level(powers)
            .into_iter()
            .filter(|power| !slotted_only || power.slots.iter().any(Option::is_some))
            .map(|power| sheet_power(input, power))
            .collect();
        if let (Some(title), false) = (title, powers.is_empty()) {
            groups.push(PowerGroup { title, powers });
        }
    };
    group(primary.clone(), &build.primary.powers, false);
    group(secondary.clone(), &build.secondary.powers, false);
    for pool in &build.pools {
        group(
            Some(naming::pool_name(&pool.id, database)),
            &pool.powers,
            false,
        );
    }
    if let Some(epic) = &build.epic_pool {
        group(
            Some(naming::pool_name(&epic.id, database)),
            &epic.powers,
            false,
        );
    }
    // An unslotted inherent is the same on every character of the archetype — the forum post's
    // rule, for the same reason.
    group(Some("Inherent powers".to_string()), &build.inherents, true);

    Sheet {
        title,
        subtitle,
        source: format!(
            "Planned with CoH Sidekick — {} data",
            build.dataset.as_str()
        ),
        stats: stat_groups(input),
        groups,
        incarnates: incarnates(input),
    }
}

/// A stable sort, so powers sharing a level keep the build's own order.
fn sorted_by_level(powers: &[SelectedPower]) -> Vec<&SelectedPower> {
    let mut sorted: Vec<&SelectedPower> = powers.iter().collect();
    sorted.sort_by_key(|power| power.level);
    sorted
}

fn sheet_power(input: &SheetInput, power: &SelectedPower) -> SheetPower {
    let levels = input
        .slot_levels
        .and_then(|levels| coh_data::slot_levels::levels_of(levels, input.build, power));
    let slots = power
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slotted)| {
            let enhancement = slotted.as_ref()?;
            Some(SheetSlot {
                level: levels.and_then(|levels| levels.get(index).copied().flatten()),
                short: short_label(enhancement),
                long: long_label(enhancement),
            })
        })
        .collect();
    SheetPower {
        level: power.level,
        name: naming::power_label(Some(input.database), &power.powerset, &power.internal_name),
        slots,
        empty_slots: power.slots.iter().filter(|slot| slot.is_none()).count(),
    }
}

fn stat_groups(input: &SheetInput) -> Vec<StatGroup> {
    StatSection::ALL
        .into_iter()
        .filter_map(|section| {
            let rows: Vec<(String, String)> = stat_registry::in_section(section)
                .filter(|stat| input.visibility.shows(stat.id))
                .filter_map(|stat| {
                    let value = (stat.read)(input.totals);
                    // Zeros are dropped, as in the post: a sheet of `0%` buries the numbers.
                    (value != 0.0).then(|| (stat.label.to_string(), stat.format.render(value)))
                })
                .collect();
            (!rows.is_empty()).then(|| StatGroup {
                title: section.title().to_string(),
                rows,
            })
        })
        .collect()
}

fn incarnates(input: &SheetInput) -> Vec<(String, String)> {
    input
        .build
        .incarnates
        .occupied()
        .map(|(slot_id, pick)| {
            let catalog = input.database.incarnate_catalog.slot(slot_id);
            let found = catalog.and_then(|slot| slot.find_power(&pick.power_name));
            let slot_label = catalog
                .map(|slot| slot.display_name.clone())
                .unwrap_or_else(|| slot_id.to_string());
            let power = found
                .map(|power| power.display_name.clone())
                .unwrap_or_else(|| pick.power_name.clone());
            let tier = found
                .map(|power| format!(" ({})", power.tier().label()))
                .unwrap_or_default();
            let state = if pick.active { "" } else { " — switched off" };
            (slot_label, format!("{power}{tier}{state}"))
        })
        .collect()
}

// ============================================================
// Enhancement labels.
// ============================================================

/// The short form of an aspect or enhancement stat, as players write it. Both vocabularies reach
/// here — set pieces carry aspect names (`Damage Resistance`), generic and origin pieces carry
/// the enhancement stat type (`Resistance`) — so both spellings are listed. An unlisted name
/// prints whole rather than guessed at.
fn abbreviate(stat: &str) -> &str {
    match stat {
        "Accuracy" => "Acc",
        "Damage" => "Dmg",
        "Recharge" => "Rech",
        "Endurance" | "EnduranceReduction" => "EndRdx",
        "EndMod" | "EnduranceModification" => "EndMod",
        "Range" => "Rng",
        "Defense" => "Def",
        "Damage Resistance" | "Resistance" => "Res",
        "Heal" | "Healing" => "Heal",
        "ToHit" => "ToHit",
        "ToHit Debuff" => "ToHitDeb",
        "Defense Debuff" => "DefDeb",
        "Immobilize" => "Immob",
        "Confuse" => "Conf",
        "Terrorize" | "Fear" => "Fear",
        "Knockback" => "KB",
        "KnockToKnockDown" => "KB>KD",
        "Run" | "Run Speed" => "Run",
        "Move Speed" => "Move",
        "InterruptTime" | "Interrupt" => "Intrpt",
        "Mez" | "Mez Duration" => "Mez",
        "Intangible" => "Intan",
        other => other,
    }
}

/// What the piece enhances, abbreviated.
///
/// A proc's own text is kept: `Chance for Smashing Damage` is what distinguishes it, and its
/// aspects (often none) do not.
fn short_label(enhancement: &Enhancement) -> String {
    match &enhancement.kind {
        EnhancementKind::IoSet {
            aspects, is_proc, ..
        } => {
            let mut parts: Vec<String> =
                aspects.iter().map(|a| abbreviate(a).to_string()).collect();
            if *is_proc {
                let name = naming::enhancement_name(enhancement);
                match name.find("Chance for") {
                    Some(at) => parts.push(name[at..].to_string()),
                    None if parts.is_empty() => parts.push(name.to_string()),
                    None => {}
                }
            }
            if parts.is_empty() {
                naming::enhancement_name(enhancement).to_string()
            } else {
                parts.join("/")
            }
        }
        EnhancementKind::GenericIo { stat, .. } | EnhancementKind::Origin { stat, .. } => {
            abbreviate(stat).to_string()
        }
        EnhancementKind::Special { .. } => naming::enhancement_name(enhancement).to_string(),
    }
}

/// The piece in full: craft level (∞ where attuned), set, and what it enhances. Generic and
/// origin pieces say which they are, since an `Acc` alone does not.
fn long_label(enhancement: &Enhancement) -> String {
    let level = if enhancement.attuned {
        "∞".to_string()
    } else {
        enhancement
            .level
            .map_or_else(String::new, |level| level.get().to_string())
    };
    let piece = match &enhancement.kind {
        EnhancementKind::IoSet { set_name, .. } if !set_name.is_empty() => {
            format!("{set_name} - {}", short_label(enhancement))
        }
        EnhancementKind::IoSet { .. } => short_label(enhancement),
        EnhancementKind::GenericIo { .. } => format!("IO {}", short_label(enhancement)),
        EnhancementKind::Origin { tier, .. } => format!("{tier} {}", short_label(enhancement)),
        EnhancementKind::Special { .. } => short_label(enhancement),
    };
    let piece = if level.is_empty() || matches!(enhancement.kind, EnhancementKind::Origin { .. }) {
        piece
    } else {
        format!("{level} {piece}")
    };
    match boost_note(enhancement) {
        Some(note) => format!("{piece} {note}"),
        None => piece,
    }
}

/// A booster or relative level, spelled as the mechanic it is ([`naming::enhancement_qualifiers`]
/// has why one field means two things).
fn boost_note(enhancement: &Enhancement) -> Option<String> {
    match &enhancement.kind {
        EnhancementKind::IoSet { .. } | EnhancementKind::GenericIo { .. } => {
            (enhancement.boost > 0).then(|| format!("+{}", enhancement.boost))
        }
        EnhancementKind::Special { .. } | EnhancementKind::Origin { .. } => {
            (enhancement.boost != 0).then(|| format!("({:+} levels)", enhancement.boost))
        }
    }
}

/// One power on one line: `Acc/Dmg(1), Dmg(3), Rech(25)`.
///
/// A comma inside a piece's own name (`Teleportation Protection, +Res(All)`) is dropped, since
/// on this line a comma is where one slot ends.
fn compact_slots(power: &SheetPower) -> String {
    power
        .slots
        .iter()
        .map(|slot| format!("{}({})", slot.short.replace(", ", " "), slot.level_text()))
        .collect::<Vec<_>>()
        .join(", ")
}

// ============================================================
// Printing.
// ============================================================

/// The preview's id — what [`print_page`] copies out.
const PREVIEW_ID: &str = "print-sheet-preview";

/// Copy the preview to `<body>` and print it. See the module docs for why the copy.
async fn print_page() -> Result<(), String> {
    let copy = format!(
        "const src = document.getElementById('{PREVIEW_ID}');
         if (!src) return false;
         let host = document.getElementById('sidekick-print');
         if (!host) {{
           host = document.createElement('div');
           host.id = 'sidekick-print';
           document.body.appendChild(host);
         }}
         host.innerHTML = src.innerHTML;
         return true;"
    );
    let copied = document::eval(&copy).await.map_err(|e| format!("{e:?}"))?;
    if copied != serde_json::Value::Bool(true) {
        return Err("the sheet was not on screen".to_string());
    }

    #[cfg(feature = "desktop")]
    {
        dioxus::desktop::window()
            .webview
            .print()
            .map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "desktop"))]
    {
        document::eval("window.print();");
        Ok(())
    }
}

/// Take the copy back out of `<body>`, so a later print from anywhere else cannot pick it up.
fn remove_print_copy() {
    document::eval("document.getElementById('sidekick-print')?.remove();");
}

// ============================================================
// The modal.
// ============================================================

#[derive(Clone, Copy)]
pub struct PrintSheetOpen(pub Signal<bool>);

/// Mounted by the shell above both layout roots, like every modal (see [`crate::modal`]).
#[component]
pub fn PrintSheetHost(database: Option<Db>) -> Element {
    let mut open = use_context::<PrintSheetOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Print the build".to_string(),
            size: ModalSize::Xl,
            on_close: move |_| open.set(false),
            PrintSheetBody { database }
        }
    }
}

#[component]
fn PrintSheetBody(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();
    let totals = use_context::<BuildTotals>().0;
    let config = use_context::<DashboardConfig>().0;

    let mut layout = use_signal(|| PrintLayout::Detailed);
    let mut with_totals = use_signal(|| true);
    let mut with_incarnates = use_signal(|| true);
    let mut failure = use_signal(|| Option::<String>::None);

    use_drop(remove_print_copy);

    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };

    let sheet = use_memo(move || {
        let build = session.build.read();
        let slot_levels = database
            .leveling_schedule
            .as_ref()
            .map(|schedule| coh_data::slot_levels::slot_levels(schedule, &build));
        build_sheet(&SheetInput {
            build: &build,
            database: &database,
            totals: &totals.read(),
            visibility: &config.read(),
            slot_levels: slot_levels.as_ref(),
        })
    });

    rsx! {
        div { class: "print-sheet-modal",
            div { class: "forum-export__controls",
                fieldset { class: "forum-export__formats",
                    legend { class: "forum-export__legend", "Layout" }
                    div { class: "forum-export__format-row",
                        for option in PrintLayout::ALL {
                            button {
                                key: "{option.id()}",
                                class: if layout() == option { "seg active" } else { "seg" },
                                r#type: "button",
                                title: "{option.hint()}",
                                "aria-pressed": layout() == option,
                                onclick: move |_| layout.set(option),
                                "{option.label()}"
                            }
                        }
                    }
                    p { class: "forum-export__hint", "{layout().hint()}" }
                }
                fieldset { class: "forum-export__sections",
                    legend { class: "forum-export__legend", "Include" }
                    label { class: "forum-export__toggle",
                        input {
                            r#type: "checkbox",
                            checked: with_totals(),
                            onchange: move |evt| with_totals.set(evt.checked()),
                        }
                        span { "Totals" }
                    }
                    label { class: "forum-export__toggle",
                        input {
                            r#type: "checkbox",
                            checked: with_incarnates(),
                            onchange: move |evt| with_incarnates.set(evt.checked()),
                        }
                        span { "Incarnates" }
                    }
                }
            }

            div { class: "print-sheet-preview",
                div { id: PREVIEW_ID,
                    SheetView {
                        sheet: sheet(),
                        layout: layout(),
                        with_totals: with_totals(),
                        with_incarnates: with_incarnates(),
                    }
                }
            }

            div { class: "forum-export__actions",
                match failure() {
                    Some(reason) => rsx! {
                        span { class: "forum-export__status is-error", "Could not print: {reason}." }
                    },
                    None => rsx! {
                        span { class: "forum-export__status is-quiet", "Opens your system's print dialog" }
                    },
                }
                button {
                    class: "seg is-primary",
                    r#type: "button",
                    onclick: move |_| async move {
                        failure.set(print_page().await.err());
                    },
                    "Print…"
                }
            }
        }
    }
}

/// The sheet itself. Styled for paper — black on white whatever the app's theme — because the
/// preview is a picture of the page.
#[component]
fn SheetView(
    sheet: Sheet,
    layout: PrintLayout,
    with_totals: bool,
    with_incarnates: bool,
) -> Element {
    let compact = layout == PrintLayout::Compact;
    rsx! {
        div { class: if compact { "print-sheet is-compact" } else { "print-sheet" },
            header { class: "print-sheet__header",
                h1 { "{sheet.title}" }
                p { class: "print-sheet__subtitle", "{sheet.subtitle}" }
                p { class: "print-sheet__source", "{sheet.source}" }
            }

            if with_totals && !sheet.stats.is_empty() {
                div { class: "print-sheet__stats",
                    for group in sheet.stats.iter() {
                        div { key: "{group.title}", class: "print-sheet__stat-group",
                            h3 { "{group.title}" }
                            table {
                                tbody {
                                    for (label, value) in group.rows.iter() {
                                        tr { key: "{label}",
                                            td { "{label}" }
                                            td { class: "val", "{value}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "print-sheet__powers",
                for group in sheet.groups.iter() {
                    section { key: "{group.title}", class: "print-sheet__group",
                        h2 { "{group.title}" }
                        for power in group.powers.iter() {
                            div { key: "{power.name}", class: "print-sheet__power",
                                if compact {
                                    p {
                                        if power.level > 0 {
                                            span { class: "print-sheet__level", "Level {power.level}: " }
                                        }
                                        span { class: "print-sheet__name", "{power.name}" }
                                        if !power.slots.is_empty() {
                                            span { class: "print-sheet__compact-slots",
                                                " — {compact_slots(power)}"
                                            }
                                        }
                                    }
                                } else {
                                    p {
                                        if power.level > 0 {
                                            span { class: "print-sheet__level", "L{power.level}" }
                                        }
                                        span { class: "print-sheet__name", "{power.name}" }
                                    }
                                    for slot in power.slots.iter() {
                                        p { class: "print-sheet__slot",
                                            span { class: "print-sheet__slot-level", "({slot.level_text()})" }
                                            " {slot.long}"
                                        }
                                    }
                                    if power.empty_slots > 0 && !power.slots.is_empty() {
                                        p { class: "print-sheet__slot is-empty",
                                            if power.empty_slots == 1 {
                                                "1 empty slot"
                                            } else {
                                                "{power.empty_slots} empty slots"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if with_incarnates && !sheet.incarnates.is_empty() {
                    section { class: "print-sheet__group",
                        h2 { "Incarnates" }
                        for (slot, power) in sheet.incarnates.iter() {
                            p { key: "{slot}", class: "print-sheet__power",
                                span { class: "print-sheet__level", "{slot}: " }
                                span { class: "print-sheet__name", "{power}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_piece(aspects: &[&str], name: &str, is_proc: bool) -> Enhancement {
        Enhancement {
            id: "x".to_string(),
            name: name.to_string(),
            icon: String::new(),
            level: coh_data::Level::new(50),
            attuned: false,
            boost: 0,
            kind: EnhancementKind::IoSet {
                set_id: "Thunderstrike".to_string(),
                set_name: "Thunderstrike".to_string(),
                piece_num: 1,
                aspects: aspects.iter().map(|a| a.to_string()).collect(),
                is_proc,
                is_unique: false,
            },
        }
    }

    #[test]
    fn a_set_piece_prints_as_its_aspects() {
        let piece = set_piece(&["Accuracy", "Damage"], "Accuracy/Damage", false);
        assert_eq!(short_label(&piece), "Acc/Dmg");
        assert_eq!(long_label(&piece), "50 Thunderstrike - Acc/Dmg");
    }

    #[test]
    fn a_proc_keeps_its_chance_text() {
        let proc = set_piece(&[], "Chance for Smashing Damage", true);
        assert_eq!(short_label(&proc), "Chance for Smashing Damage");
        let mixed = set_piece(&["Damage"], "Damage/Chance for Fire Damage", true);
        assert_eq!(short_label(&mixed), "Dmg/Chance for Fire Damage");
    }

    #[test]
    fn a_generic_io_names_its_stat() {
        let piece = Enhancement {
            kind: EnhancementKind::GenericIo {
                stat: "EnduranceReduction".to_string(),
                value: 0.0,
            },
            ..set_piece(&[], "Endurance Reduction IO", false)
        };
        assert_eq!(short_label(&piece), "EndRdx");
        assert_eq!(long_label(&piece), "50 IO EndRdx");
    }

    #[test]
    fn compact_line_reads_like_the_request() {
        let slot = |short: &str, level| SheetSlot {
            level,
            short: short.to_string(),
            long: String::new(),
        };
        let power = SheetPower {
            level: 1,
            name: "Spectral Wounds".to_string(),
            slots: vec![
                slot("Dmg", Some(1)),
                slot("Acc", Some(3)),
                slot("Dmg", None),
                slot("Teleportation Protection, +Res(All)", Some(46)),
            ],
            empty_slots: 0,
        };
        assert_eq!(
            compact_slots(&power),
            "Dmg(1), Acc(3), Dmg(?), Teleportation Protection +Res(All)(46)"
        );
    }
}
