//! Build image export — the build as a PNG poster.
//!
//! Draws the build to a pixel buffer using the `image` crate — no DOM, no rasterization step.
//! The card is [`CARD_WIDTH`] CSS px wide and the output resolution is this width times a scale
//! factor. It draws powers as tiles, stats as cards, and set bonuses as text, lettered in SN Pro
//! ([`crate::export_text`]).
//!
//! The modal is a Dioxus component with controls on the left and a preview on the right.
//! Download and Copy rasterize the card via [`render_to_png`].

use crate::export_text::{Lettering, Weight};
use crate::modal::{Modal, ModalSize};
use crate::naming;
use crate::panels::dashboards::Dashboards;
use crate::panels::powers::build_set_bonus_block;
use crate::panels::stat_registry::{self, StatSection};
use crate::power_art::PowerArt;
use crate::shell::Db;
use coh_data::{CharacterState, SelectedPower};
use coh_math::CalculatedTotals;
use dioxus::prelude::*;
use image::{Rgba, RgbaImage};

// ============================================================
// Options model.
// ============================================================

/// Preset detail levels — Compact / Standard / Full.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportPreset {
    Compact,
    Standard,
    Full,
}

/// Which stat sections to include. Mirrors the React `ALL_STAT_SECTIONS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatSectionId {
    Offense,
    SurvivalAndMobility,
    StealthAndPerception,
    Defense,
    DamageResistance,
    StatusProtection,
    StatusEffectResistance,
    DebuffResistance,
}

impl StatSectionId {
    pub fn all() -> &'static [StatSectionId] {
        &[
            StatSectionId::Offense,
            StatSectionId::SurvivalAndMobility,
            StatSectionId::StealthAndPerception,
            StatSectionId::Defense,
            StatSectionId::DamageResistance,
            StatSectionId::StatusProtection,
            StatSectionId::StatusEffectResistance,
            StatSectionId::DebuffResistance,
        ]
    }

    pub fn compact() -> &'static [StatSectionId] {
        &[
            StatSectionId::Offense,
            StatSectionId::SurvivalAndMobility,
            StatSectionId::Defense,
            StatSectionId::DamageResistance,
        ]
    }
}

/// User-facing options for the export.
#[derive(Clone, Debug)]
pub struct ExportImageOptions {
    pub preset: ExportPreset,
    pub author_name: String,
    pub show_level: bool,
    pub show_credit: bool,
    pub show_enhancements: bool,
    pub show_inherents: bool,
    pub show_incarnates: bool,
    pub only_slotted: bool,
    pub stat_sections: Vec<StatSectionId>,
    pub show_set_bonuses: bool,
    pub transparent: bool,
    pub scale: u32,
}

impl Default for ExportImageOptions {
    /// What the Standard preset sets, since Standard is the one shown selected. The two used to
    /// disagree, so the poster first drawn was not the one the highlighted preset draws.
    fn default() -> Self {
        let mut options = Self {
            preset: ExportPreset::Standard,
            author_name: String::new(),
            show_level: true,
            show_credit: true,
            show_enhancements: true,
            show_inherents: true,
            show_incarnates: true,
            only_slotted: false,
            stat_sections: Vec::new(),
            show_set_bonuses: false,
            transparent: false,
            scale: 2,
        };
        options.apply_preset(ExportPreset::Standard);
        options
    }
}

impl ExportImageOptions {
    /// Apply a preset's defaults, preserving identity + appearance choices.
    pub fn apply_preset(&mut self, preset: ExportPreset) {
        self.preset = preset;
        match preset {
            ExportPreset::Compact => {
                self.stat_sections = Vec::from(StatSectionId::compact());
                self.only_slotted = true;
                self.show_set_bonuses = false;
            }
            ExportPreset::Standard => {
                self.stat_sections = Vec::from(StatSectionId::all());
                self.only_slotted = true;
                self.show_set_bonuses = true;
            }
            ExportPreset::Full => {
                self.stat_sections = Vec::from(StatSectionId::all());
                self.only_slotted = false;
                self.show_set_bonuses = true;
            }
        }
    }
}

// ============================================================
// Constants.
// ============================================================

/// Fixed render width in CSS px. Output resolution = this × the scale option.
pub const CARD_WIDTH: usize = 1200;
/// Minimum height for a roughly 16:9 landscape.
pub const CARD_MIN_HEIGHT: usize = (CARD_WIDTH * 9) / 16;

/// Working canvas height before cropping. A full 24-power build with every stat section on
/// measures well under this; anything taller is clipped rather than silently mis-laid-out.
const CANVAS_MAX_HEIGHT: usize = 6000;
/// Card padding.
const PAD: usize = 24;
/// Power tile dimensions: the tile, the power's art in it, and the marks under its name.
const TILE_H: usize = 52;
const TILE_GAP: usize = 6;
const TILE_ICON: usize = 36;
const SLOT_ICON: usize = 20;
const SLOT_DOT: usize = 9;
/// Columns of power tiles, stat cards and set-bonus cards on the poster.
const POWER_COLS: usize = 4;
const STAT_COLS: usize = 4;
const BONUS_COLS: usize = 3;
/// Space left under each band of the poster.
const ZONE_GAP: usize = 14;
/// Title band above a stat card's first row, and the pitch of the rows under it.
/// Gap left around text inside a stat card.
const CARD_PAD: usize = 6;
const STAT_CARD_GAP: usize = 8;
/// The poster's text sizes at 1x, in CSS pixels of SN Pro ([`crate::export_text`]): the build's
/// name (bold), the archetype line, a stat card's title, and everything else.
const POSTER_TITLE: usize = 30;
const POSTER_SUBTITLE: usize = 20;
const STAT_TITLE: usize = 12;
const POSTER_TEXT: usize = 11;

// ============================================================
// Colors.
// ============================================================

// The unused members below are marked rather than deleted: these are one Tailwind step each
// (emerald-400, purple-400, red-500) and a contiguous slate scale, so the set is the unit. A
// renderer that gains a band picks its colour from here; a scale with holes in it sends the
// next person to pick a hex by eye.
const BG_COLOR: Rgba<u8> = Rgba([20, 20, 30, 255]);
#[allow(dead_code)]
const CARD_BG: Rgba<u8> = Rgba([15, 15, 25, 255]);
const BORDER_COLOR: Rgba<u8> = Rgba([60, 60, 80, 200]);
const TEXT_COLOR: Rgba<u8> = Rgba([230, 230, 240, 255]);
const SUBTITLE_COLOR: Rgba<u8> = Rgba([180, 200, 220, 255]);
const ZONE_LABEL_COLOR: Rgba<u8> = Rgba([160, 170, 190, 255]);
const TILE_BG: Rgba<u8> = Rgba([40, 40, 55, 200]);
const TILE_BORDER: Rgba<u8> = Rgba([70, 70, 90, 180]);
const STAT_CARD_BG: Rgba<u8> = Rgba([35, 35, 50, 220]);
const EMERALD: Rgba<u8> = Rgba([52, 211, 153, 255]);
const CYAN: Rgba<u8> = Rgba([103, 232, 249, 255]);
const AMBER: Rgba<u8> = Rgba([251, 191, 36, 255]);
const PURPLE: Rgba<u8> = Rgba([192, 132, 252, 255]);
#[allow(dead_code)]
const RED: Rgba<u8> = Rgba([239, 68, 68, 255]);
const SLATE_500: Rgba<u8> = Rgba([100, 116, 139, 255]);
const SLATE_400: Rgba<u8> = Rgba([148, 163, 184, 255]);
const SLATE_600: Rgba<u8> = Rgba([71, 85, 105, 255]);
const SLATE_700: Rgba<u8> = Rgba([51, 65, 85, 255]);
const SLATE_800: Rgba<u8> = Rgba([30, 41, 59, 255]);
/// A tile's level badge, dark enough to read over any power's art.
const BADGE_BG: Rgba<u8> = Rgba([15, 23, 42, 230]);
/// Behind a power's ON tag (emerald-900), so ON reads green at a glance and OFF stays quiet.
const ON_TAG_BG: Rgba<u8> = Rgba([6, 78, 59, 255]);

// ============================================================
// Simple text renderer.
// ============================================================

// ============================================================
// Rendering.
// ============================================================

/// Render the build poster to a PNG byte buffer.
///
/// Takes the build, the database (for power names), the totals (for stats),
/// and the user's options. Returns the PNG bytes or an error string.
///
/// Everything is laid out in 1x units and multiplied by the scale, text included. The bitmap
/// font only scales in whole steps, so a text size that is not a multiple of 8 drew at a
/// different proportion at each resolution: at 2x the tile names drew three glyphs tall in tiles
/// laid out for two, and were cut to six letters.
pub fn render_to_png(
    build: &CharacterState,
    database: &Db,
    totals: &CalculatedTotals,
    visibility: &Dashboards,
    options: &ExportImageOptions,
    // The build's power art, already decoded. Gathered by `power_art::load_for` before this
    // call, because gathering it is async on the web and this function is neither async nor
    // allowed to be — it is a pure function of its inputs, which is what lets the preview and
    // both export buttons share one render.
    art: &PowerArt,
) -> Result<Vec<u8>, String> {
    let s = options.scale.max(1) as usize;
    let width = CARD_WIDTH * s;
    // Generous working height; the image is cropped to the drawn extent before encoding, so
    // over-allocating here costs a moment of memory and nothing in the output.
    let height = CANVAS_MAX_HEIGHT * s;
    let mut img = RgbaImage::new(width as u32, height as u32);
    // A fresh buffer is fully transparent, which is the transparent background already.
    if !options.transparent {
        fill_rect(&mut img, 0, 0, width, height, BG_COLOR);
    }

    let pad = PAD * s;
    let inner_w = width - pad * 2;
    let body = POSTER_TEXT * s;
    let gap = TILE_GAP * s;
    let mut y = pad;

    // Header: name, archetype and sets, author — and the level hard right.
    let level_text = options.show_level.then(|| format!("Level {}", build.level));
    let level_w = level_text
        .as_ref()
        .map_or(0, |text| text_width(text, POSTER_SUBTITLE * s) + pad);
    if let Some(text) = &level_text {
        let x = width - pad - text_width(text, POSTER_SUBTITLE * s);
        draw_text(&mut img, x, y, SLATE_400, POSTER_SUBTITLE * s, text);
    }
    let name = build.name.trim();
    if !name.is_empty() {
        let title = Lettering::new((POSTER_TITLE * s) as f32, Weight::Bold, TEXT_COLOR);
        let shown = title.fit(name, inner_w.saturating_sub(level_w));
        title.draw(&mut img, pad, y, &shown);
        y += text_height(POSTER_TITLE * s) + 8 * s;
    }
    let archetype = naming::archetype_name(build, database);
    let sets: Vec<String> = [&build.primary, &build.secondary]
        .into_iter()
        .filter_map(|selection| naming::powerset_name(selection, database))
        .collect();
    let mut x = pad;
    if let Some(archetype) = &archetype {
        let shown = truncate_to(archetype, POSTER_SUBTITLE * s, inner_w);
        draw_text(&mut img, x, y, CYAN, POSTER_SUBTITLE * s, &shown);
        x += text_width(&shown, POSTER_SUBTITLE * s);
    }
    if !sets.is_empty() {
        let lead = if archetype.is_some() { " — " } else { "" };
        let text = format!("{lead}{}", sets.join(" / "));
        let room = (pad + inner_w).saturating_sub(x);
        let shown = truncate_to(&text, POSTER_SUBTITLE * s, room);
        draw_text(&mut img, x, y, SLATE_400, POSTER_SUBTITLE * s, &shown);
    }
    y += text_height(POSTER_SUBTITLE * s) + 6 * s;
    let author = options.author_name.trim();
    if !author.is_empty() {
        let text = truncate_to(&format!("by {author}"), body, inner_w);
        draw_text(&mut img, pad, y, SLATE_400, body, &text);
        y += text_height(body) + 6 * s;
    }
    // What the totals assumed beyond the build itself, so the image alone says how its numbers
    // were read. Nothing is drawn for a build at the planner's defaults.
    let assumed = assumptions(build, database);
    if !assumed.is_empty() {
        let lead = "SETTINGS  ";
        draw_text(&mut img, pad, y, ZONE_LABEL_COLOR, body, lead);
        let indent = text_width(lead, body);
        for line in wrap_items(&assumed, body, inner_w - indent) {
            draw_text(&mut img, pad + indent, y, AMBER, body, &line);
            y += text_height(body) + 4 * s;
        }
        y += 2 * s;
    }
    y += 4 * s;
    fill_rect(&mut img, pad, y, inner_w, s, BORDER_COLOR);
    y += 12 * s;

    // The power grid, shared by the three tile bands.
    let tile_w = (inner_w - (POWER_COLS - 1) * gap) / POWER_COLS;
    let tile_h = TILE_H * s;
    let grid_cell = |top: usize, i: usize| {
        (
            pad + (i % POWER_COLS) * (tile_w + gap),
            top + (i / POWER_COLS) * (tile_h + gap),
        )
    };
    let grid_bottom = |top: usize, count: usize| {
        top + count.div_ceil(POWER_COLS) * (tile_h + gap) - gap + ZONE_GAP * s
    };

    // Powers, in the order the character took them. Inherents are their own band below, so a
    // power is drawn once — the poster used to draw every inherent in both.
    let kept = |power: &&SelectedPower| !options.only_slotted || is_slotted(power);
    let mut powers: Vec<&SelectedPower> = build
        .all_selected()
        .filter(|power| power.inherent_category.is_none())
        .filter(kept)
        .collect();
    // Stable, so powers sharing a level keep the build's own order (primary, secondary, pools).
    powers.sort_by_key(|power| power.level);
    let tiles = TileStyle {
        scale: s,
        show_enhancements: options.show_enhancements,
        origin: build.origin.as_deref(),
        build,
    };
    if !powers.is_empty() {
        y = zone_label(&mut img, y, "Powers by level", s);
        for (i, power) in powers.iter().enumerate() {
            let (tx, ty) = grid_cell(y, i);
            draw_power_tile(&mut img, tx, ty, tile_w, power, &tiles, database, art);
        }
        y = grid_bottom(y, powers.len());
    }

    if options.show_inherents {
        let inherents: Vec<&SelectedPower> = build
            .all_selected()
            .filter(|power| power.inherent_category.is_some())
            .filter(kept)
            .collect();
        if !inherents.is_empty() {
            y = zone_label(&mut img, y, "Inherent powers", s);
            for (i, power) in inherents.iter().enumerate() {
                let (tx, ty) = grid_cell(y, i);
                draw_power_tile(&mut img, tx, ty, tile_w, power, &tiles, database, art);
            }
            y = grid_bottom(y, inherents.len());
        }
    }

    if options.show_incarnates {
        let picks: Vec<_> = build.incarnates.occupied().collect();
        if !picks.is_empty() {
            y = zone_label(&mut img, y, "Incarnates", s);
            for (i, (slot_id, pick)) in picks.iter().enumerate() {
                let (tx, ty) = grid_cell(y, i);
                draw_incarnate_tile(&mut img, tx, ty, tile_w, slot_id, pick, database, s);
            }
            y = grid_bottom(y, picks.len());
        }
    }

    // Totals, in up to four columns, each card placed under the shortest column so far. A
    // section whose every row the dashboard hides is left off: on a poster an empty "No stats
    // shown" card says nothing about the build.
    let sections: Vec<(StatSection, usize)> = options
        .stat_sections
        .iter()
        .filter_map(|id| stat_section_to_stat_section(*id))
        .map(|section| {
            let rows = stat_registry::in_section(section)
                .filter(|stat| visibility.shows(stat.id))
                .count();
            (section, rows)
        })
        .filter(|(_, rows)| *rows > 0)
        .collect();
    if !sections.is_empty() {
        y = zone_label(&mut img, y, "Character totals", s);
        let cols = sections.len().min(STAT_COLS);
        let card_gap = STAT_CARD_GAP * s;
        let card_w = (inner_w - (cols - 1) * card_gap) / cols;
        let mut tops = vec![y; cols];
        for (section, rows) in sections {
            let col = shortest(&tops);
            let x = pad + col * (card_w + card_gap);
            draw_stat_section(
                &mut img, x, tops[col], card_w, &section, totals, visibility, s,
            );
            tops[col] += stat_card_height(rows, s) + card_gap;
        }
        y = tops.into_iter().max().unwrap_or(y) - card_gap + ZONE_GAP * s;
    }

    if options.show_set_bonuses {
        let cards = set_bonus_cards(build, database, totals);
        if !cards.is_empty() {
            y = zone_label(&mut img, y, "Active set bonuses", s);
            let cols = cards.len().min(BONUS_COLS);
            let card_gap = STAT_CARD_GAP * s;
            let card_w = (inner_w - (cols - 1) * card_gap) / cols;
            let mut tops = vec![y; cols];
            for card in &cards {
                let col = shortest(&tops);
                let x = pad + col * (card_w + card_gap);
                let drawn = draw_set_bonus_card(&mut img, x, tops[col], card_w, card, s);
                tops[col] += drawn + card_gap;
            }
            y = tops.into_iter().max().unwrap_or(y) - card_gap + ZONE_GAP * s;
        }
    }

    // The footer sits under the content, or at the foot of the minimum card when the content is
    // short. It used to be drawn at the bottom of the working canvas and then cropped away.
    let min_height = CARD_MIN_HEIGHT * s;
    let bottom = if options.show_credit {
        let line = text_height(body);
        let footer_y = (y + 4 * s).max(min_height.saturating_sub(pad + line));
        fill_rect(&mut img, pad, footer_y - 8 * s, inner_w, s, BORDER_COLOR);
        draw_text(
            &mut img,
            pad,
            footer_y,
            SLATE_500,
            body,
            "Made with coh-sidekick.com",
        );
        let date = today_utc();
        let date_x = width - pad - text_width(&date, body);
        draw_text(&mut img, date_x, footer_y, SLATE_500, body, &date);
        footer_y + line + pad
    } else {
        (y.saturating_sub(ZONE_GAP * s) + pad).max(min_height)
    };

    let used = bottom.min(height);
    let img = image::imageops::crop_imm(&img, 0, 0, width as u32, used as u32).to_image();
    encode_plain_png(&img)
}

/// What the totals assumed beyond the build itself: the combat settings off their defaults, the
/// form each stance power is in, the what-if team buffs and the proc kinds switched off. Empty for
/// a build read at the planner's defaults.
fn assumptions(build: &CharacterState, database: &Db) -> Vec<String> {
    let mut parts = crate::panels::combat::drift_summary(&build.combat);
    for group in coh_data::caster_state::stance_groups(build, &database.0) {
        let Some(active) = &group.active else {
            continue;
        };
        let name = group
            .options
            .iter()
            .find(|option| &option.internal_name == active)
            .map_or(active.as_str(), |option| option.name.as_str());
        parts.push(format!("{}: {name}", group.parent_name));
    }
    let what_if = crate::panels::what_if::summary(&build.combat.what_if_buffs);
    if !what_if.is_empty() {
        parts.push(format!("What-if {}", what_if.join(", ")));
    }
    if !build.disabled_proc_categories.is_empty() {
        let off: Vec<&str> = build
            .disabled_proc_categories
            .iter()
            .map(String::as_str)
            .collect();
        parts.push(format!("Procs off: {}", off.join(", ")));
    }
    parts
}

/// `items` joined with ` · `, as the app joins them, into lines no wider than `room`. An item
/// too wide for a line of its own is cut to it.
fn wrap_items(items: &[String], size: usize, room: usize) -> Vec<String> {
    const SEP: &str = " · ";
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for item in items {
        let item = truncate_to(item, size, room);
        let joined = format!("{line}{SEP}{item}");
        if line.is_empty() {
            line = item;
        } else if text_width(&joined, size) <= room {
            line = joined;
        } else {
            lines.push(std::mem::replace(&mut line, item));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Whether anything is slotted in the power — what "only slotted powers" keeps.
fn is_slotted(power: &SelectedPower) -> bool {
    power.slots.iter().any(Option::is_some)
}

/// The index of the shortest column, for placing the next card under it.
fn shortest(tops: &[usize]) -> usize {
    tops.iter()
        .enumerate()
        .min_by_key(|(_, top)| **top)
        .map_or(0, |(col, _)| col)
}

/// A band's small uppercase label. Returns where the band's content starts.
fn zone_label(img: &mut RgbaImage, y: usize, label: &str, s: usize) -> usize {
    let size = POSTER_TEXT * s;
    draw_text(
        img,
        PAD * s,
        y,
        ZONE_LABEL_COLOR,
        size,
        &label.to_uppercase(),
    );
    y + text_height(size) + 6 * s
}

/// One line of text at `(x, y)`.
fn draw_text(img: &mut RgbaImage, x: usize, y: usize, color: Rgba<u8>, size: usize, text: &str) {
    lettering(size, color).draw(img, x, y, text);
}

// THE EXPORTED IMAGE NO LONGER CARRIES THE BUILD INSIDE IT, by decision on 2026-09-26. An
// `encode_png` stood here that differed from [`encode_plain_png`] below in one respect: it called
// `coh_data::skif::encode` and wrote the result into a PNG iTXt chunk keyed `skif`
// (`SKIF_CHUNK_KEYWORD`), so every poster the planner produced held a complete, invisible copy of
// the build. Recoverable from `git show 30cd8e918:crates/app/src/export_image.rs`.
//
// NOTHING HAS EVER READ IT. Not this app -- `skif::decode` is reached only from
// `build_file.rs:149`, on text -- and not the beta, where `skif` names the pasteable build code
// and no PNG is parsed anywhere. Importing a build works, and only works, through the text door:
// a pasted code or a link (`build_io::read_pasted`, `build_io::open_route`,
// `build_file::decode_import`). Image import was never planned, and the decision is that the
// export stays one-way.
//
// The doc that stood here said "the poster is both the picture you post and the file someone loads
// back", and the menu hint said "PNG of the build with the build embedded in it". Both were the
// only places the round trip was ever claimed, and both are gone with it -- the second matters
// beyond tidiness, because a shared poster was handing its recipient the whole build without
// saying so.
//
// With the chunk gone the two encoders were the same function, so there is now one:
// [`encode_plain_png`].

/// Encode a canvas as a PNG, carrying the picture and nothing else.
///
/// The one encoder for both the exported poster and the social preview card, since 2026-09-26 --
/// see the note above for what the poster used to carry and why it stopped. An image is not a
/// document here: nothing loads one back into the planner, so nothing is stored in one.
pub(crate) fn encode_plain_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, img.width(), img.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG encode failed: {e}"))?;
        writer
            .write_image_data(img.as_raw())
            .map_err(|e| format!("PNG encode failed: {e}"))?;
    }
    Ok(buf)
}

/// Convert our StatSectionId to the real StatSection.
fn stat_section_to_stat_section(id: StatSectionId) -> Option<StatSection> {
    match id {
        StatSectionId::Offense => Some(StatSection::Offense),
        StatSectionId::SurvivalAndMobility => Some(StatSection::Survival),
        StatSectionId::StealthAndPerception => Some(StatSection::Movement),
        StatSectionId::Defense => Some(StatSection::Defense),
        StatSectionId::DamageResistance => Some(StatSection::Resistance),
        StatSectionId::StatusProtection => Some(StatSection::StatusProtection),
        StatSectionId::StatusEffectResistance => Some(StatSection::StatusResistance),
        StatSectionId::DebuffResistance => Some(StatSection::DebuffResistance),
    }
}

/// What every power tile on one poster shares.
struct TileStyle<'a> {
    scale: usize,
    show_enhancements: bool,
    /// The character's origin, which an origin enhancement's frame carries.
    origin: Option<&'a str>,
    /// The build, for each power's ON/OFF switch.
    build: &'a CharacterState,
}

/// Draw a power tile: the power's art with its level on it, the name beside it, and under the
/// name one mark per slot — the slotted enhancement's own icon, or a dot when the options ask
/// for the compact form.
#[allow(clippy::too_many_arguments)]
fn draw_power_tile(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    power: &SelectedPower,
    style: &TileStyle,
    database: &Db,
    art: &PowerArt,
) {
    let s = style.scale;
    let h = TILE_H * s;
    fill_rect(img, x, y, w, h, TILE_BG);
    stroke_rect(img, x, y, w, h, TILE_BORDER);

    let pad = CARD_PAD * s;
    let icon_size = TILE_ICON * s;

    // One lookup answers both halves of the tile: which picture it wears and what it is called.
    let def =
        crate::view::power_view::resolve_power_def(database, &power.powerset, &power.internal_name);

    // The power's art leads the tile, when we have it. An icon that did not load costs the tile
    // its picture and nothing else (see `power_art`); the gutter stays, so the names in a column
    // of tiles line up either way.
    let icon = def
        .and_then(|def| def.extra.get("icon"))
        .and_then(|value| value.as_str())
        .and_then(|name| art.get(Some(name)));
    match icon {
        Some(source) => draw_image_scaled(img, source, x + pad, y + pad, icon_size, icon_size),
        None => stroke_rect(img, x + pad, y + pad, icon_size, icon_size, TILE_BORDER),
    }

    // The level, as a badge on the art's corner.
    if power.level > 0 {
        let size = POSTER_TEXT * s;
        let text = power.level.to_string();
        let badge_w = text_width(&text, size) + 4 * s;
        let badge_h = text_height(size) + 2 * s;
        fill_rect(img, x + pad / 2, y + pad / 2, badge_w, badge_h, BADGE_BG);
        stroke_rect(img, x + pad / 2, y + pad / 2, badge_w, badge_h, SLATE_600);
        draw_text(
            img,
            x + pad / 2 + 2 * s,
            y + pad / 2 + s,
            SLATE_400,
            size,
            &text,
        );
    }

    // Tags down the tile's right edge: the power's ON/OFF switch as its planner card shows it,
    // then the targets hit or stacks counted where the power takes a count. A count on a power
    // switched off counts nothing, so it is left off, and so is a stack count nobody set: one
    // stack is just the power.
    let size = POSTER_TEXT * s;
    let switch = crate::panels::powers::pill_state(database, style.build, power);
    let count = def
        .filter(|_| switch != Some(false))
        .and_then(coh_math::stacking::stacking_slider)
        .and_then(|slider| {
            let n = slider.effective(power.targets_hit);
            match slider.kind {
                coh_math::stacking::SliderKind::Targets => Some(format!("{n} hit")),
                coh_math::stacking::SliderKind::Stacks => {
                    power.targets_hit.map(|_| format!("x{n}"))
                }
            }
        });
    let tags: Vec<(String, Rgba<u8>, Rgba<u8>)> = [
        switch.map(|on| {
            if on {
                ("ON".to_string(), ON_TAG_BG, EMERALD)
            } else {
                ("OFF".to_string(), BADGE_BG, SLATE_500)
            }
        }),
        count.map(|text| (text, BADGE_BG, SLATE_400)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut tag_y = y + pad / 2;
    let mut tags_w = 0;
    for (text, bg, fg) in &tags {
        let tag_w = text_width(text, size) + 4 * s;
        let tag_h = text_height(size) + 2 * s;
        let tag_x = x + w - pad / 2 - tag_w;
        fill_rect(img, tag_x, tag_y, tag_w, tag_h, *bg);
        stroke_rect(img, tag_x, tag_y, tag_w, tag_h, SLATE_600);
        draw_text(img, tag_x + 2 * s, tag_y + s, *fg, size, text);
        tag_y += tag_h + 2 * s;
        tags_w = tags_w.max(tag_w + pad / 2);
    }

    // The name gets everything right of the art, short of the tags, and is cut to it.
    let text_x = x + pad + icon_size + pad;
    let room = (x + w).saturating_sub(text_x + pad.max(tags_w + 2 * s));
    let name = truncate_to(tile_label(power, database), size, room);
    draw_text(img, text_x, y + pad, TEXT_COLOR, size, &name);

    let slots_y = y + pad + text_height(size) + 3 * s;
    if style.show_enhancements {
        let cell = SLOT_ICON * s;
        for (i, slot) in power.slots.iter().enumerate() {
            let cx = text_x + i * (cell + 3 * s);
            if cx + cell > x + w {
                break;
            }
            draw_slot_icon(img, cx, slots_y, cell, slot.as_ref(), style, database, art);
        }
    } else {
        let dot = SLOT_DOT * s;
        for (i, slot) in power.slots.iter().enumerate() {
            let cx = text_x + i * (dot + 3 * s);
            let colour = if slot.is_some() { EMERALD } else { SLATE_700 };
            fill_circle(img, cx, slots_y + 4 * s, dot, colour);
        }
    }
}

/// One slot under a power's name: the enhancement's icon under its rarity frame, as the power
/// card draws it, or an empty outline. A piece whose art did not load is a plain filled square,
/// so the slot still reads as filled.
#[allow(clippy::too_many_arguments)]
fn draw_slot_icon(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    size: usize,
    slot: Option<&coh_data::Enhancement>,
    style: &TileStyle,
    database: &Db,
    art: &PowerArt,
) {
    let Some(enhancement) = slot else {
        fill_rect(img, x, y, size, size, SLATE_800);
        stroke_rect(img, x, y, size, size, SLATE_600);
        return;
    };
    let base = crate::panels::powers::slot_icon_url(enhancement, database)
        .and_then(|url| art.enhancement(&url));
    match base {
        Some(base) => {
            draw_image_scaled(img, base, x, y, size, size);
            let frame =
                crate::panels::powers::slot_overlay_url(enhancement, database, style.origin);
            if let Some(frame) = art.enhancement(&frame) {
                draw_image_scaled(img, frame, x, y, size, size);
            }
        }
        None => {
            fill_rect(img, x, y, size, size, SLATE_700);
            stroke_rect(img, x, y, size, size, EMERALD);
        }
    }
}

/// A filled circle `d` across, its bounding box's top-left at `(x, y)`.
fn fill_circle(img: &mut RgbaImage, x: usize, y: usize, d: usize, color: Rgba<u8>) {
    let r2 = (d * d) as f32 / 4.0;
    let centre = d as f32 / 2.0;
    for dy in 0..d {
        for dx in 0..d {
            let (fx, fy) = (dx as f32 + 0.5 - centre, dy as f32 + 0.5 - centre);
            if fx * fx + fy * fy <= r2 {
                fill_rect(img, x + dx, y + dy, 1, 1, color);
            }
        }
    }
}

/// What a tile calls its power: the name a reader knows it by.
///
/// The poster used to print the export's `internalName` — `Stone_Armor` where the card beside it
/// says Rock Armor, `Earths_Embrace` for Earth's Embrace. Nothing caught it because nothing read
/// the def here at all until the icon needed one, and the only test that drew a tile was the
/// ignored viewer, whose fixture did not resolve either (see `a_stone_build`).
///
/// A power whose def will not resolve keeps its internal name rather than going blank: the raw
/// string is what we actually know about it, and a tile with no words at all would read as a
/// power the build does not have (Rule 1).
fn tile_label<'a>(power: &'a SelectedPower, database: &'a Db) -> &'a str {
    crate::view::power_view::resolve_power_def(database, &power.powerset, &power.internal_name)
        .map_or(power.internal_name.as_str(), |def| def.name.as_str())
}

/// Composite `source` into `img` at `(x, y)`, scaled to `w` x `h`, over whatever is already
/// there.
///
/// Nearest-neighbour, because every input is a 32x32 icon drawn at very near 32 px and a
/// filtered resample would only soften art that was authored for this size. Alpha is a real
/// `over` blend rather than a copy: these icons carry transparent corners, and copying would
/// stamp the tile's rounded background out into hard black squares.
///
/// Anything that would land outside the canvas is skipped, so a tile near the crop edge draws
/// what fits instead of panicking on a bounds check.
pub(crate) fn draw_image_scaled(
    img: &mut RgbaImage,
    source: &RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    if w == 0 || h == 0 || source.width() == 0 || source.height() == 0 {
        return;
    }
    for row in 0..h {
        let dest_y = (y + row) as u32;
        if dest_y >= img.height() {
            break;
        }
        // Map the destination pixel back to a source pixel. `min` guards the last row/column
        // against the rounding that would otherwise index one past the edge.
        let src_y = ((row * source.height() as usize) / h).min(source.height() as usize - 1) as u32;
        for col in 0..w {
            let dest_x = (x + col) as u32;
            if dest_x >= img.width() {
                break;
            }
            let src_x =
                ((col * source.width() as usize) / w).min(source.width() as usize - 1) as u32;
            let src = source.get_pixel(src_x, src_y).0;
            if src[3] == 0 {
                continue;
            }
            if src[3] == 255 {
                img.put_pixel(dest_x, dest_y, Rgba(src));
                continue;
            }
            let dst = img.get_pixel(dest_x, dest_y).0;
            let a = src[3] as u32;
            let inv = 255 - a;
            let blend = |s: u8, d: u8| ((s as u32 * a + d as u32 * inv) / 255) as u8;
            img.put_pixel(
                dest_x,
                dest_y,
                Rgba([
                    blend(src[0], dst[0]),
                    blend(src[1], dst[1]),
                    blend(src[2], dst[2]),
                    // The destination keeps the stronger alpha: on a transparent export the
                    // icon is what makes its pixels opaque, and on an opaque one this is 255
                    // either way.
                    dst[3].max(src[3]),
                ]),
            );
        }
    }
}

/// Draw an incarnate tile: the power's name, and the slot it fills under it. Named the way the
/// incarnate panel names it, from the catalog, with the stored internal name as the fallback.
#[allow(clippy::too_many_arguments)]
fn draw_incarnate_tile(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    slot_id: &str,
    pick: &coh_data::IncarnateSlot,
    database: &Db,
    s: usize,
) {
    let h = TILE_H * s;
    fill_rect(img, x, y, w, h, TILE_BG);
    stroke_rect(img, x, y, w, h, TILE_BORDER);

    let catalog = database.incarnate_catalog.slot(slot_id);
    let name = catalog
        .and_then(|slot| slot.find_power(&pick.power_name))
        .map_or(pick.power_name.as_str(), |power| {
            power.display_name.as_str()
        });
    let slot_label = catalog.map_or(slot_id, |slot| slot.display_name.as_str());

    let pad = CARD_PAD * s;
    let size = POSTER_TEXT * s;
    let room = w.saturating_sub(pad * 2);
    draw_text(
        img,
        x + pad,
        y + pad,
        PURPLE,
        size,
        &truncate_to(name, size, room),
    );
    let label_y = y + pad + text_height(size) + 6 * s;
    draw_text(
        img,
        x + pad,
        label_y,
        SLATE_400,
        size,
        &truncate_to(slot_label, size, room),
    );
}

/// One set's bonuses in one power, as the poster lists them.
struct SetBonusCard {
    title: String,
    power: String,
    /// Each active bonus's description, and whether the Rule of 5 refuses it.
    effects: Vec<(String, bool)>,
}

/// Every set in the build that has reached a bonus, per power — the forum export's set-bonus
/// section, drawn. The tiers and their Rule-of-5 verdicts come from the same
/// [`build_set_bonus_block`] the slot tooltip uses, so the poster cannot advertise a bonus the
/// game refuses: a refused one is drawn struck through.
fn set_bonus_cards(
    build: &CharacterState,
    database: &Db,
    totals: &CalculatedTotals,
) -> Vec<SetBonusCard> {
    let Some(catalog) = database.io_sets.as_ref() else {
        return Vec::new();
    };
    let mut cards = Vec::new();
    for power in build.all_selected() {
        let mut set_ids: Vec<&str> = Vec::new();
        for slot in power.slots.iter().flatten() {
            if let coh_data::EnhancementKind::IoSet { set_id, .. } = &slot.kind {
                if !set_ids.contains(&set_id.as_str()) {
                    set_ids.push(set_id);
                }
            }
        }
        for set_id in set_ids {
            let Some(set) = catalog.get(set_id) else {
                continue;
            };
            let block = build_set_bonus_block(set, power, set_id, &totals.set_bonus_tracking);
            let effects: Vec<(String, bool)> = block
                .tiers
                .iter()
                .filter(|tier| tier.active)
                .flat_map(|tier| tier.effects.iter())
                .map(|effect| (effect.description.clone(), effect.refused))
                .collect();
            if effects.is_empty() {
                continue;
            }
            let set_name = if set.name.is_empty() {
                set_id
            } else {
                set.name.as_str()
            };
            cards.push(SetBonusCard {
                title: format!("{set_name} x{}", block.slotted),
                power: naming::power_label(Some(database), &power.powerset, &power.internal_name),
                effects,
            });
        }
    }
    cards
}

/// Draw one set-bonus card; returns its height. Descriptions wrap to the card rather than
/// being cut, since the end of one is the number.
fn draw_set_bonus_card(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    card: &SetBonusCard,
    s: usize,
) -> usize {
    let pad = CARD_PAD * s;
    let size = POSTER_TEXT * s;
    let line = text_height(size) + 3 * s;
    let room = w.saturating_sub(pad * 2);
    let lines: Vec<(String, bool)> = card
        .effects
        .iter()
        .flat_map(|(text, refused)| {
            wrap(text, size, room)
                .into_iter()
                .map(move |part| (part, *refused))
        })
        .collect();
    let h = pad * 2 + line * 2 + 3 * s + lines.len() * line;
    fill_rect(img, x, y, w, h, STAT_CARD_BG);
    stroke_rect(img, x, y, w, h, BORDER_COLOR);

    let mut ty = y + pad;
    draw_text(
        img,
        x + pad,
        ty,
        EMERALD,
        size,
        &truncate_to(&card.title, size, room),
    );
    ty += line;
    draw_text(
        img,
        x + pad,
        ty,
        SLATE_500,
        size,
        &truncate_to(&card.power, size, room),
    );
    ty += line + 3 * s;
    for (text, refused) in lines {
        if refused {
            draw_text(img, x + pad, ty, SLATE_500, size, &text);
            let strike_y = ty + text_height(size) / 2;
            fill_rect(
                img,
                x + pad,
                strike_y,
                text_width(&text, size),
                s,
                SLATE_500,
            );
        } else {
            draw_text(img, x + pad, ty, SLATE_400, size, &text);
        }
        ty += line;
    }
    h
}

/// `text` broken into lines of at most `max_width`, at word boundaries. A single word longer
/// than a line is cut with [`truncate_to`].
fn wrap(text: &str, size: usize, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let joined = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if line.is_empty() || text_width(&joined, size) <= max_width {
            line = joined;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
        .into_iter()
        .map(|line| truncate_to(&line, size, max_width))
        .collect()
}

/// Draw a stat section card.
fn draw_stat_section(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    section: &StatSection,
    totals: &CalculatedTotals,
    visibility: &Dashboards,
    scale: usize,
) {
    let rows: Vec<_> = stat_registry::in_section(*section)
        .filter(|stat| visibility.shows(stat.id))
        // The plain value, without a per-second rate: the card truncates the label to fit the
        // value, and "+371.2% · 61.32 HP/s" cut "Regeneration" to "Regen...".
        .map(|stat| {
            let mut resolved = stat.resolve(totals);
            resolved.text = stat.format.render((stat.read)(totals));
            (stat.label, resolved)
        })
        .collect();

    let height = stat_card_height(rows.len(), scale);
    fill_rect(img, x, y, w, height, STAT_CARD_BG);
    stroke_rect(img, x, y, w, height, BORDER_COLOR);

    let pad = CARD_PAD * scale;
    draw_text(
        img,
        x + pad,
        y + pad,
        TEXT_COLOR,
        STAT_TITLE * scale,
        section.title(),
    );

    let size = POSTER_TEXT * scale;
    let row_h = stat_row_height(scale);
    for (i, (label, resolved)) in rows.iter().enumerate() {
        let row_y = y + stat_card_header(scale) + i * row_h;
        let value_w_hint = text_width(&resolved.text, size) + pad * 4;
        let label_space = w.saturating_sub(value_w_hint);
        let shown = truncate_to(label, size, label_space);
        draw_text(img, x + pad, row_y, SLATE_400, size, &shown);

        // The value is right-aligned, so its start depends on how wide it draws.
        let text = if resolved.simulated {
            // The dashboard marks simulated numbers so a screenshot of a buffed build cannot
            // pass as the build's own. An exported poster IS that screenshot, so it carries the
            // same mark rather than dropping it.
            format!("{} sim", resolved.text)
        } else {
            resolved.text.clone()
        };
        let value_w = text_width(&text, size);
        let value_x = (x + w).saturating_sub(pad + value_w);
        let colour = if resolved.at_cap {
            AMBER
        } else {
            SUBTITLE_COLOR
        };
        draw_text(img, value_x, row_y, colour, size, &text);
    }

    if rows.is_empty() {
        let top = y + stat_card_header(scale);
        draw_text(img, x + pad, top, SLATE_500, size, "No stats shown");
    }
}

/// The title band above a card's first row: the title itself plus padding either side.
fn stat_card_header(scale: usize) -> usize {
    CARD_PAD * scale + text_height(STAT_TITLE * scale) + CARD_PAD * scale
}

/// The pitch of a card's rows.
fn stat_row_height(scale: usize) -> usize {
    text_height(POSTER_TEXT * scale) + CARD_PAD * scale
}

/// How tall a card with `rows` rows draws, so the layout can reserve it before drawing.
fn stat_card_height(rows: usize, scale: usize) -> usize {
    stat_card_header(scale) + rows.max(1) * stat_row_height(scale) + CARD_PAD * scale
}

/// Regular SN Pro at `size` pixels.
fn lettering(size: usize, color: Rgba<u8>) -> Lettering {
    Lettering::new(size as f32, Weight::Regular, color)
}

/// How wide `text` draws at `size`; right-aligning a value depends on it.
fn text_width(text: &str, size: usize) -> usize {
    lettering(size, TEXT_COLOR).width(text)
}

/// How tall a line drawn at `size` is. Row pitch follows it, or rows overlap.
fn text_height(size: usize) -> usize {
    lettering(size, TEXT_COLOR).height()
}

/// `text` cut to fit `max_width`, ending in `…` when it had to cut. A name that silently ran
/// past its tile collided with the level label beside it.
fn truncate_to(text: &str, size: usize, max_width: usize) -> String {
    lettering(size, TEXT_COLOR).fit(text, max_width)
}

/// Fill a rectangle with a solid color.
pub(crate) fn fill_rect(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: Rgba<u8>,
) {
    for py in y..y.saturating_add(h) {
        for px in x..x.saturating_add(w) {
            if px < img.width() as usize && py < img.height() as usize {
                img.put_pixel(px as u32, py as u32, color);
            }
        }
    }
}

/// Draw a rectangle outline.
fn stroke_rect(img: &mut RgbaImage, x: usize, y: usize, w: usize, h: usize, color: Rgba<u8>) {
    // Top edge.
    for px in x..x.saturating_add(w) {
        if px < img.width() as usize && y < img.height() as usize {
            img.put_pixel(px as u32, y as u32, color);
        }
    }
    // Bottom edge.
    let bottom_y = y + h - 1;
    for px in x..x.saturating_add(w) {
        if px < img.width() as usize && bottom_y < img.height() as usize {
            img.put_pixel(px as u32, bottom_y as u32, color);
        }
    }
    // Left edge.
    for py in y..y.saturating_add(h) {
        if x < img.width() as usize && py < img.height() as usize {
            img.put_pixel(x as u32, py as u32, color);
        }
    }
    // Right edge.
    let right_x = x + w - 1;
    for py in y..y.saturating_add(h) {
        if right_x < img.width() as usize && py < img.height() as usize {
            img.put_pixel(right_x as u32, py as u32, color);
        }
    }
}

// ============================================================
// Modal.
// ============================================================

/// The export image modal.
#[component]
pub fn ExportImageHost(database: Option<Db>) -> Element {
    let mut open = use_context::<ExportImageOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Export as image".to_string(),
            size: ModalSize::Xl,
            on_close: move |_| open.set(false),
            ExportImageBody { database }
        }
    }
}

/// The export's open state, held at the shell root for the containment reason every modal here
/// shares (see [`crate::modal`]).
#[derive(Clone, Copy)]
pub struct ExportImageOpen(pub Signal<bool>);

#[component]
fn ExportImageBody(database: Option<Db>) -> Element {
    let session = use_context::<crate::build_session::BuildSession>();
    let totals = use_context::<crate::panels::stats::BuildTotals>().0;
    let config = use_context::<crate::panels::dashboards::DashboardConfig>().0;
    let mut saved = use_signal(|| Option::<Result<crate::build_file::WrittenTo, String>>::None);
    let mut copied = use_signal(|| Option::<Result<(), String>>::None);

    let mut options = use_signal(ExportImageOptions::default);
    // A changed option is a different poster, so the last save or copy no longer describes it.
    use_effect(move || {
        let _ = options.read();
        saved.set(None);
        copied.set(None);
    });
    let Some(database) = database else {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    };

    // Derived, not stored: the poster is a function of the build, the totals, the visible stats
    // and the options, and derived state that is STORED is state that can be stale. It is also
    // computed once for the preview and both buttons rather than three times.
    //
    // A resource rather than a memo, because the power art has to be gathered before the render
    // and gathering it is async on the web (see `power_art`). The render itself stays the pure
    // synchronous function it was; only the wait moved out here. That the whole thing is one
    // resource — gather THEN draw — is what keeps a half-drawn poster from being exportable:
    // until the art is in, there are no bytes at all, so there is no window in which Download
    // could hand back a build whose icons had not arrived yet.
    let png = use_resource({
        let database = database.clone();
        move || {
            // Read on the outside of the future, so the resource is reactive on all four and
            // re-renders when any of them moves.
            let database = database.clone();
            let build = session.build.read().clone();
            let totals = totals.read().clone();
            let config = config.read().clone();
            let options = options.read().clone();
            async move {
                let art = crate::power_art::load_for(&build, &database).await;
                render_to_png(&build, &database, &totals, &config, &options, &art)
            }
        }
    });
    let preview_src = png
        .read()
        .as_ref()
        .and_then(|rendered| rendered.as_ref().ok())
        .map(|bytes| format!("data:image/png;base64,{}", crate::clipboard::base64(bytes)));
    // Only a finished render is exportable; a pending one leaves both buttons off rather than
    // offering bytes that do not exist yet.
    let ready = matches!(&*png.read(), Some(Ok(_)));
    let file_name = format!(
        "{}.png",
        crate::build_file::file_name_for(&session.build.read(), &database)
            .trim_end_matches(".skif")
    );

    rsx! {
        div { class: "image-export",
            div { class: "image-export__controls",
                fieldset { class: "image-export__group",
                    legend { class: "image-export__legend", "Detail" }
                    div { class: "image-export__row",
                        for (preset , label) in [
                            (ExportPreset::Compact, "Compact"),
                            (ExportPreset::Standard, "Standard"),
                            (ExportPreset::Full, "Full"),
                        ]
                        {
                            button {
                                key: "{label}",
                                r#type: "button",
                                class: if options.read().preset == preset { "seg active" } else { "seg" },
                                "aria-pressed": options.read().preset == preset,
                                onclick: move |_| options.write().apply_preset(preset),
                                "{label}"
                            }
                        }
                    }
                }

                div { class: "image-export__group",
                    label { class: "image-export__legend", r#for: "image-export-author",
                        "Author or character"
                    }
                    input {
                        id: "image-export-author",
                        class: "sb-save__input",
                        r#type: "text",
                        value: options.read().author_name.clone(),
                        oninput: move |evt| options.write().author_name = evt.value().to_string(),
                        placeholder: "Optional credit",
                    }
                }

                fieldset { class: "image-export__group",
                    legend { class: "image-export__legend", "Include" }
                    ImageToggle {
                        label: "Level",
                        checked: options.read().show_level,
                        on_toggle: move |on| options.write().show_level = on,
                    }
                    ImageToggle {
                        label: "Enhancement icons",
                        checked: options.read().show_enhancements,
                        on_toggle: move |on| options.write().show_enhancements = on,
                    }
                    ImageToggle {
                        label: "Inherent powers",
                        checked: options.read().show_inherents,
                        on_toggle: move |on| options.write().show_inherents = on,
                    }
                    ImageToggle {
                        label: "Incarnates",
                        checked: options.read().show_incarnates,
                        on_toggle: move |on| options.write().show_incarnates = on,
                    }
                    ImageToggle {
                        label: "Only slotted powers",
                        checked: options.read().only_slotted,
                        on_toggle: move |on| options.write().only_slotted = on,
                    }
                    ImageToggle {
                        label: "Set bonuses",
                        checked: options.read().show_set_bonuses,
                        on_toggle: move |on| options.write().show_set_bonuses = on,
                    }
                    ImageToggle {
                        label: "Credit footer",
                        checked: options.read().show_credit,
                        on_toggle: move |on| options.write().show_credit = on,
                    }
                    ImageToggle {
                        label: "Transparent background",
                        checked: options.read().transparent,
                        on_toggle: move |on| options.write().transparent = on,
                    }
                }

                fieldset { class: "image-export__group",
                    legend { class: "image-export__legend", "Resolution" }
                    div { class: "image-export__row",
                        for scale in [1u32, 2] {
                            button {
                                key: "{scale}",
                                r#type: "button",
                                class: if options.read().scale == scale { "seg active" } else { "seg" },
                                "aria-pressed": options.read().scale == scale,
                                onclick: move |_| options.write().scale = scale,
                                "{scale}x"
                            }
                        }
                    }
                }
            }

            div { class: "image-export__main",
                div { class: "image-export__frame",
                    match (&*png.read(), &preview_src) {
                        (Some(Ok(_)), Some(src)) => rsx! {
                            img { src: "{src}", alt: "The build as a poster" }
                        },
                        (Some(Err(why)), _) => rsx! {
                            div { class: "load-state error", "Could not render: {why}" }
                        },
                        // Pending. Its own state, and not the error state: on the web the
                        // first poster of a session waits on a few dozen icon fetches, and
                        // a red "could not render" is a lie about a render still going.
                        (None, _) => rsx! {
                            div { class: "load-state", "Drawing the poster…" }
                        },
                        _ => rsx! {},
                    }
                }

                div { class: "image-export__actions",
                    // Both outcomes are reported. A copy button that claims success while the
                    // clipboard still holds what it held before is the soft-wrong outcome the
                    // clipboard module exists to prevent, and the user finds out by pasting.
                    span { class: "image-export__status",
                        match (&*saved.read(), &*copied.read(), &*png.read()) {
                            (Some(Err(why)), _, _) => rsx! {
                                span { class: "is-error", "Could not save: {why}" }
                            },
                            (_, Some(Err(why)), _) => rsx! {
                                span { class: "is-error", "Could not copy: {why}" }
                            },
                            (Some(Ok(Some(path))), _, _) => rsx! { "Saved to {path}" },
                            (Some(Ok(None)), _, _) => rsx! { "Saved." },
                            (_, Some(Ok(())), _) => rsx! { "Copied to the clipboard." },
                            (_, _, Some(Ok(bytes))) => rsx! {
                                span { class: "is-quiet", "{bytes.len() / 1024} KB" }
                            },
                            _ => rsx! {},
                        }
                    }
                    button {
                        r#type: "button",
                        class: "seg",
                        disabled: !ready,
                        // The copy runs in this handler's own task: both clipboard
                        // mechanisms are gated on a live user gesture, and the gesture is
                        // over by the time a later task would run.
                        onclick: move |_| {
                            let bytes = png.read().clone();
                            async move {
                                saved.set(None);
                                match bytes {
                                    Some(Ok(bytes)) => copied.set(Some(crate::clipboard::copy_png(&bytes).await)),
                                    Some(Err(why)) => copied.set(Some(Err(why))),
                                    // Unreachable while the button is gated on `ready`.
                                    None => {}
                                }
                            }
                        },
                        "Copy to clipboard"
                    }
                    button {
                        r#type: "button",
                        class: "seg is-primary",
                        disabled: !ready,
                        onclick: {
                            let file_name = file_name.clone();
                            move |_| {
                                let file_name = file_name.clone();
                                let bytes = png.read().clone();
                                async move {
                                    copied.set(None);
                                    match bytes {
                                        Some(Ok(bytes)) => saved.set(Some(
                                            crate::build_file::write_png_file(&file_name, &bytes).await,
                                        )),
                                        Some(Err(why)) => saved.set(Some(Err(why))),
                                        // Unreachable while the button is gated on `ready`.
                                        None => {}
                                    }
                                }
                            }
                        },
                        "Download PNG"
                    }
                }
            }
        }
    }
}

/// One include/exclude switch.
#[component]
fn ImageToggle(label: String, checked: bool, on_toggle: EventHandler<bool>) -> Element {
    rsx! {
        label { class: "image-export__toggle",
            input {
                r#type: "checkbox",
                checked,
                onchange: move |evt| on_toggle.call(evt.checked()),
            }
            span { "{label}" }
        }
    }
}

/// Entry point from the build menu.
#[component]
pub fn ExportImageEntry(database: Option<Db>) -> Element {
    let mut export = use_context::<ExportImageOpen>().0;
    let mut menu = use_context::<crate::popover::PopoverOpen>().0;

    rsx! {
        button {
            r#type: "button",
            class: "main-menu__item",
            disabled: database.is_none(),
            onclick: move |_| {
                export.set(true);
                menu.set(false);
            },
            span { class: "main-menu__label", "Export as image…" }
            span { class: "main-menu__hint", "PNG of the build, to post or save" }
        }
    }
}

/// Today's date for the footer, in UTC. Read through `web_time` because `chrono::Local::now()`
/// panics on `wasm32-unknown-unknown`, which has no clock of its own — opening this modal on the
/// web crashed the app, since the preview draws the footer as soon as it mounts.
fn today_utc() -> String {
    let at_ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|at| at.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{DatasetId, PowerDatabase};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn database() -> Db {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contract/homecoming/bundle.json.gz");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        Db(Arc::new(
            PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads"),
        ))
    }

    /// A Homecoming Scrapper with toggles and attacks, imported as the planner imports it.
    fn maiden_fury(database: &Db) -> CharacterState {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/buildsave/homecoming/maidenfury.txt");
        let text = std::fs::read_to_string(&path).expect("fixture reads");
        let mut build = crate::build_file::decode_import(&text, &database.0, DatasetId::Homecoming)
            .expect("fixture imports")
            .build;
        crate::inherents::sync(&mut build, &database.0);
        crate::granted_powers::sync(&mut build, &database.0);
        build
    }

    fn poster(build: &CharacterState, database: &Db) -> Vec<u8> {
        let totals = coh_math::recalculate(build, &database.0);
        let options = ExportImageOptions::default();
        render_to_png(
            build,
            database,
            &totals,
            &Dashboards::default(),
            &options,
            &PowerArt::default(),
        )
        .expect("renders")
    }

    #[test]
    fn a_build_at_the_planners_defaults_assumes_nothing() {
        let database = database();
        let mut build = maiden_fury(&database);
        build.combat = coh_data::CombatContext::default();
        build.disabled_proc_categories.clear();
        assert_eq!(assumptions(&build, &database), Vec::<String>::new());
    }

    #[test]
    fn off_default_settings_are_listed_with_their_values() {
        let database = database();
        let mut build = maiden_fury(&database);
        build.combat = coh_data::CombatContext::default();
        build.combat.in_combat = true;
        build.combat.fury_level = 80.0;
        build.combat.exemplar_level = coh_data::Level::new(30);
        build
            .combat
            .what_if_buffs
            .insert("damage".to_string(), 20.0);
        build.disabled_proc_categories.insert("Defense".to_string());
        let assumed = assumptions(&build, &database);
        for expected in ["In combat", "Fury 80", "Exemplar 30", "Procs off: Defense"] {
            assert!(
                assumed.iter().any(|part| part == expected),
                "{expected} missing from {assumed:?}"
            );
        }
        assert!(
            assumed
                .iter()
                .any(|part| part.starts_with("What-if ") && part.ends_with("+20%")),
            "what-if missing from {assumed:?}"
        );
    }

    /// The tile's tag is the card's pill: a toggle shows one that follows `is_active`, an attack
    /// shows none.
    #[test]
    fn the_tile_tag_follows_the_cards_pill() {
        let database = database();
        let mut build = maiden_fury(&database);
        let states: Vec<(String, Option<bool>)> = build
            .all_selected()
            .map(|power| {
                let state = crate::panels::powers::pill_state(&database, &build, power);
                (power.internal_name.clone(), state)
            })
            .collect();
        assert!(
            states.iter().any(|(_, state)| state.is_none()),
            "{states:?}"
        );
        let (toggle, _) = states
            .iter()
            .find(|(name, state)| state.is_some() && name == "Focused_Fighting")
            .or_else(|| states.iter().find(|(_, state)| state.is_some()))
            .expect("the build has a power with a pill");
        for on in [false, true] {
            build
                .all_selected_mut()
                .find(|power| &power.internal_name == toggle)
                .expect("still held")
                .is_active = on;
            let power = build
                .all_selected()
                .find(|power| &power.internal_name == toggle)
                .expect("still held");
            assert_eq!(
                crate::panels::powers::pill_state(&database, &build, power),
                Some(on),
                "{toggle}"
            );
        }
    }

    /// One tile drawn alone, so nothing but the tile itself can differ between two draws.
    fn tile(build: &CharacterState, database: &Db, power: &SelectedPower) -> RgbaImage {
        let mut img = RgbaImage::new(300, TILE_H as u32);
        let style = TileStyle {
            scale: 1,
            show_enhancements: false,
            origin: None,
            build,
        };
        draw_power_tile(
            &mut img,
            0,
            0,
            300,
            power,
            &style,
            database,
            &PowerArt::default(),
        );
        img
    }

    /// A toggle's tile draws differently on and off; an attack's has no tag to change.
    #[test]
    fn the_tile_draws_its_switch() {
        let database = database();
        let mut build = maiden_fury(&database);
        let toggle = build
            .all_selected()
            .find(|power| crate::panels::powers::pill_state(&database, &build, power).is_some())
            .map(|power| power.internal_name.clone())
            .expect("the build has a power with a pill");
        let mut drawn = Vec::new();
        for on in [false, true] {
            build
                .all_selected_mut()
                .find(|power| power.internal_name == toggle)
                .expect("still held")
                .is_active = on;
            let power = build
                .all_selected()
                .find(|power| power.internal_name == toggle)
                .expect("still held");
            drawn.push(tile(&build, &database, power));
        }
        assert_ne!(drawn[0], drawn[1], "{toggle} draws the same on and off");
    }

    /// The settings line is drawn: a build in combat makes a taller poster than one at defaults.
    #[test]
    fn the_poster_draws_its_settings() {
        let database = database();
        let mut build = maiden_fury(&database);
        build.combat = coh_data::CombatContext::default();
        build.disabled_proc_categories.clear();
        let height = |png: Vec<u8>| image::load_from_memory(&png).expect("decodes").height();
        let plain = height(poster(&build, &database));
        build.combat.in_combat = true;
        let assumed = height(poster(&build, &database));
        assert!(assumed > plain, "{assumed} vs {plain}");
    }

    #[test]
    fn settings_wrap_to_the_room_given() {
        let items: Vec<String> = (0..12).map(|i| format!("Setting number {i}")).collect();
        let room = 300;
        let lines = wrap_items(&items, 8, room);
        assert!(lines.len() > 1);
        assert!(
            lines.iter().all(|line| text_width(line, 8) <= room),
            "{lines:?}"
        );
        assert_eq!(lines.join(" | ").matches("Setting").count(), 12);
    }

    /// Writes the poster at each preset to look at: `PREVIEW_BUILD=<build file> cargo test -p app
    /// --features desktop poster_sample -- --ignored`, into `target/poster-<preset>.png` (or under
    /// `PREVIEW_OUT_DIR`). With no `PREVIEW_BUILD` it draws the Maiden Fury fixture.
    #[test]
    #[ignore = "writes pictures to look at; run with --ignored"]
    fn poster_sample() {
        let database = database();
        let build = match std::env::var_os("PREVIEW_BUILD") {
            None => maiden_fury(&database),
            Some(path) => {
                let text = std::fs::read_to_string(&path).expect("build file reads");
                let mut build =
                    crate::build_file::decode_import(&text, &database.0, DatasetId::Homecoming)
                        .expect("build imports")
                        .build;
                crate::inherents::sync(&mut build, &database.0);
                crate::granted_powers::sync(&mut build, &database.0);
                build
            }
        };
        let art = {
            let mut future = std::pin::pin!(crate::power_art::load_for(&build, &database));
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            match std::future::Future::poll(future.as_mut(), &mut context) {
                std::task::Poll::Ready(art) => art,
                std::task::Poll::Pending => panic!("native art loading waited on something"),
            }
        };
        let totals = coh_math::recalculate(&build, &database.0);
        let dir = std::env::var_os("PREVIEW_OUT_DIR").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        );
        for (name, preset) in [
            ("compact", ExportPreset::Compact),
            ("standard", ExportPreset::Standard),
            ("full", ExportPreset::Full),
        ] {
            let mut options = ExportImageOptions::default();
            options.apply_preset(preset);
            let png = render_to_png(
                &build,
                &database,
                &totals,
                &Dashboards::default(),
                &options,
                &art,
            )
            .expect("renders");
            std::fs::write(dir.join(format!("poster-{name}.png")), png).expect("the poster writes");
        }
    }
}
