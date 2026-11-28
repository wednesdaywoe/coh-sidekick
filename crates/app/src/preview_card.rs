//! The share preview card: the 1200x880 picture a chat app shows for a shared build's link.
//!
//! A port of the beta's v6 `BuildPreviewCard.tsx`, measure for measure. It is sized for Discord,
//! which shows it at about a third of full size, and the beta's header notes say why each choice
//! survives that: every power as its icon and slots, every defense and resistance as a bar, four
//! headline numbers, the slotted inherents, and the site's name. Like the poster it carries no
//! build inside it ([`encode_plain_png`]), and every number is read off the planner's totals.

use crate::export_image::{draw_image_scaled, encode_plain_png, fill_rect};
use crate::export_text::{self, Lettering, Weight};
use crate::naming;
use crate::panels::stat_registry::{self, StatCap};
use crate::power_art::PowerArt;
use crate::shell::Db;
use coh_data::{CharacterState, Enhancement, SelectedPower};
use coh_math::CalculatedTotals;
use image::{imageops, Rgba, RgbaImage};

/// The card's fixed size. `backfill-preview` shape-checks every upload against exactly this box,
/// so this is the client half of a server contract, not a styling choice.
pub const PREVIEW_CARD_WIDTH: usize = 1200;
pub const PREVIEW_CARD_HEIGHT: usize = 880;

const PAD: usize = 28;
const CONTENT_W: usize = PREVIEW_CARD_WIDTH - 2 * PAD;
const CONTENT_RIGHT: usize = PREVIEW_CARD_WIDTH - PAD;

// The beta's Tailwind colours, by their Tailwind names.
const GRAY_950: Rgba<u8> = Rgba([3, 7, 18, 255]);
const SLATE_50: Rgba<u8> = Rgba([248, 250, 252, 255]);
const SLATE_200: Rgba<u8> = Rgba([226, 232, 240, 255]);
const SLATE_300: Rgba<u8> = Rgba([203, 213, 225, 255]);
const SLATE_400: Rgba<u8> = Rgba([148, 163, 184, 255]);
const SLATE_500: Rgba<u8> = Rgba([100, 116, 139, 255]);
const SLATE_700: Rgba<u8> = Rgba([51, 65, 85, 255]);
const SLATE_800: Rgba<u8> = Rgba([30, 41, 59, 255]);
const CYAN_200: Rgba<u8> = Rgba([165, 243, 252, 255]);
const CYAN_300: Rgba<u8> = Rgba([103, 232, 249, 255]);
const CYAN_400: Rgba<u8> = Rgba([34, 211, 238, 255]);
const AMBER_300: Rgba<u8> = Rgba([252, 211, 77, 255]);
const AMBER_500: Rgba<u8> = Rgba([245, 158, 11, 255]);
const GREEN_400: Rgba<u8> = Rgba([74, 222, 128, 255]);
const EMERALD_300: Rgba<u8> = Rgba([110, 231, 183, 255]);
const RED_400: Rgba<u8> = Rgba([248, 113, 113, 255]);

/// Header: the name's line, the archetype line under it, and the gap to the rule.
const TITLE_LINE: f32 = 60.0;
const SUBTITLE_LINE: f32 = 33.0;
const HEADER_GAP: usize = 8;

/// One defense or resistance bar: its initial, then a track whose fill is the value.
const BAR_LETTER_W: usize = 16;
const BAR_LETTER_GAP: usize = 6;
const BAR_TRACK_W: usize = 100;
const BAR_TRACK_H: usize = 14;
const BAR_ROW_H: usize = 20;
const BAR_ROW_GAP: usize = 4;
const BAR_COL_GAP: usize = 12;
const BAR_BLOCK_GAP: usize = 24;
const BAR_TITLE_LINE: f32 = 27.0;
const BAR_TITLE_GAP: usize = 4;
/// A soft-capped track runs this far past its cap, with a tick at the cap, so a build on the
/// softcap and one well past it do not draw as the same full bar.
const SOFT_OVERFLOW: f64 = 1.4;

/// The four headline tiles, two by two at the band's right.
const TILES_W: usize = 400;
const TILE_GAP: usize = 8;
const TILE_W: usize = (TILES_W - TILE_GAP) / 2;
const TILE_LABEL_LINE: f32 = 27.5;
const TILE_VALUE_LINE: f32 = 47.5;
const TILE_PAD_Y: usize = 4;
const TILE_H: usize = 2 + 2 * TILE_PAD_Y + 75;

/// One power: its icon, then its slots in a row beside it.
const POWER_ICON: usize = 46;
const SLOT_ICON: usize = 40;
const SLOT_GAP: usize = 3;
const ICON_TO_SLOTS: usize = 8;
const POWER_ROW_GAP: usize = 6;
const POWERS_PER_COLUMN: usize = 8;
const POWERS_TOP_GAP: usize = 12;

/// The bottom two bands, measured up from the card's foot.
const FOOTER_LINE: usize = 30;
const FOOTER_GAP: usize = 6;
const INHERENT_GAP: usize = 16;
const INHERENT_PAD_TOP: usize = 8;

const TYPED_DEFENSE: [&str; 8] = [
    "def_smashing",
    "def_lethal",
    "def_fire",
    "def_cold",
    "def_energy",
    "def_negative",
    "defense_psionic",
    "defense_toxic",
];
const POSITIONAL_DEFENSE: [&str; 3] = ["defense_melee", "defense_ranged", "defense_aoe"];
const RESISTANCE: [&str; 8] = [
    "res_smashing",
    "res_lethal",
    "res_fire",
    "res_cold",
    "res_energy",
    "res_negative",
    "res_psionic",
    "res_toxic",
];

/// Draw the share preview card for a build.
///
/// `art` is the build's power and enhancement art, gathered by [`crate::power_art::load_for`]
/// first, because gathering it is async on the web and this function is not.
pub fn render_preview_png(
    build: &CharacterState,
    database: &Db,
    totals: &CalculatedTotals,
    art: &PowerArt,
) -> Result<Vec<u8>, String> {
    let mut img = RgbaImage::from_pixel(
        PREVIEW_CARD_WIDTH as u32,
        PREVIEW_CARD_HEIGHT as u32,
        GRAY_950,
    );
    let rule_y = draw_header(&mut img, build, database);
    let band_bottom = draw_stat_band(&mut img, rule_y + 1 + HEADER_GAP, totals);

    let footer_text_top = PREVIEW_CARD_HEIGHT - PAD - FOOTER_LINE;
    let footer_rule = footer_text_top - FOOTER_GAP - 1;
    let inherents: Vec<&SelectedPower> = build
        .all_selected()
        .filter(|power| power.inherent_category.is_some() && is_slotted(power))
        .collect();
    let powers_bottom = if inherents.is_empty() {
        footer_rule - FOOTER_GAP
    } else {
        let row_top = footer_rule - FOOTER_GAP - POWER_ICON;
        let rule = row_top - INHERENT_PAD_TOP - 1;
        fill_rect(&mut img, PAD, rule, CONTENT_W, 1, SLATE_800);
        draw_inherent_row(&mut img, row_top, &inherents, build, database, art);
        rule
    };
    let row = Row {
        build,
        database,
        art,
    };
    draw_power_columns(&mut img, band_bottom + POWERS_TOP_GAP, powers_bottom, &row);

    fill_rect(&mut img, PAD, footer_rule, CONTENT_W, 1, SLATE_800);
    let footer = Lettering::new(20.0, Weight::Regular, SLATE_500);
    draw_in_line(
        &mut img,
        &footer,
        PAD,
        footer_text_top as f32,
        FOOTER_LINE as f32,
        "coh-sidekick.com",
    );

    encode_plain_png(&img)
}

/// The name, the level at the right, the archetype and sets under them, and the rule below.
/// Returns the rule's y.
fn draw_header(img: &mut RgbaImage, build: &CharacterState, database: &Db) -> usize {
    let top = PAD as f32;
    let level = Lettering::new(24.0, Weight::Regular, SLATE_300);
    let level_text = format!("Level {}", build.level);
    let level_w = level.width(&level_text);
    draw_in_line(img, &level, CONTENT_RIGHT - level_w, top, 36.0, &level_text);

    let title = Lettering::new(48.0, Weight::Bold, SLATE_50);
    let name = title.fit(build.name.trim(), CONTENT_W - level_w - 16);
    draw_in_line(img, &title, PAD, top, TITLE_LINE, &name);

    let line = top + TITLE_LINE;
    let mut x = PAD;
    if let Some(archetype) = naming::archetype_name(build, database) {
        let lettering = Lettering::new(26.0, Weight::Regular, CYAN_300);
        let shown = lettering.fit(&archetype, CONTENT_W);
        x += draw_in_line(img, &lettering, x, line, SUBTITLE_LINE, &shown);
    }
    let sets: Vec<String> = [&build.primary, &build.secondary]
        .into_iter()
        .filter_map(|selection| naming::powerset_name(selection, database))
        .collect();
    if !sets.is_empty() {
        let lettering = Lettering::new(26.0, Weight::Regular, SLATE_400);
        let lead = if x > PAD { " — " } else { "" };
        let text = format!("{lead}{}", sets.join(" / "));
        let shown = lettering.fit(&text, CONTENT_RIGHT.saturating_sub(x));
        draw_in_line(img, &lettering, x, line, SUBTITLE_LINE, &shown);
    }

    let rule_y = (line + SUBTITLE_LINE).round() as usize + HEADER_GAP;
    fill_rect(img, PAD, rule_y, CONTENT_W, 1, SLATE_700);
    rule_y
}

/// The bar matrix at the left and the headline tiles at the right. Returns the band's bottom.
fn draw_stat_band(img: &mut RgbaImage, top: usize, totals: &CalculatedTotals) -> usize {
    let defense = BarHue {
        fill: CYAN_400,
        capped: CYAN_200,
    };
    let resist = BarHue {
        fill: AMBER_500,
        capped: AMBER_300,
    };
    let blocks = [
        ("Defense", bars(&TYPED_DEFENSE, totals), 2, defense),
        ("Position", bars(&POSITIONAL_DEFENSE, totals), 1, defense),
        ("Resist", bars(&RESISTANCE, totals), 2, resist),
    ];
    let mut x = PAD;
    let mut bottom = top;
    for (title, bars, cols, hue) in &blocks {
        if bars.is_empty() {
            continue;
        }
        let (width, block_bottom) = draw_bar_block(img, x, top, title, bars, *cols, *hue);
        x += width + BAR_BLOCK_GAP;
        bottom = bottom.max(block_bottom);
    }

    let tiles = headline_tiles(totals);
    let left = CONTENT_RIGHT - TILES_W;
    for (i, (label, value, color)) in tiles.iter().enumerate() {
        let tile_x = left + (i % 2) * (TILE_W + TILE_GAP);
        let tile_y = top + (i / 2) * (TILE_H + TILE_GAP);
        draw_tile(img, tile_x, tile_y, label, value, *color);
        bottom = bottom.max(tile_y + TILE_H);
    }
    bottom
}

#[derive(Clone, Copy)]
struct BarHue {
    fill: Rgba<u8>,
    capped: Rgba<u8>,
}

/// One def/res value with the ceiling its bar is scaled against.
struct Bar {
    letter: String,
    value: f64,
    ceiling: f64,
    soft: bool,
}

/// The bars for `ids`, in order. A stat with no ceiling from the engine gets no bar: a bar with
/// nothing to scale against would be decoration.
fn bars(ids: &[&str], totals: &CalculatedTotals) -> Vec<Bar> {
    ids.iter()
        .filter_map(|id| {
            let def = stat_registry::by_id(id)?;
            Some(Bar {
                letter: def.label.chars().next()?.to_string(),
                value: (def.read)(totals),
                ceiling: def.cap.ceiling(totals)?,
                soft: def.cap == StatCap::DefenseSoftcap,
            })
        })
        .collect()
}

/// One titled block of bars, filling down `cols` columns. Returns its width and bottom.
fn draw_bar_block(
    img: &mut RgbaImage,
    x: usize,
    top: usize,
    title: &str,
    bars: &[Bar],
    cols: usize,
    hue: BarHue,
) -> (usize, usize) {
    let heading = Lettering::new(18.0, Weight::Regular, SLATE_500);
    draw_in_line(
        img,
        &heading,
        x,
        top as f32,
        BAR_TITLE_LINE,
        &title.to_uppercase(),
    );
    let bars_top = top + BAR_TITLE_LINE as usize + BAR_TITLE_GAP;
    let rows = bars.len().div_ceil(cols);
    let col_w = BAR_LETTER_W + BAR_LETTER_GAP + BAR_TRACK_W;
    for (i, bar) in bars.iter().enumerate() {
        let bar_x = x + (i / rows) * (col_w + BAR_COL_GAP);
        let bar_y = bars_top + (i % rows) * (BAR_ROW_H + BAR_ROW_GAP);
        draw_bar(img, bar_x, bar_y, bar, hue);
    }
    let width = cols * col_w + (cols - 1) * BAR_COL_GAP;
    let bottom = bars_top + rows * BAR_ROW_H + (rows - 1) * BAR_ROW_GAP;
    (width, bottom)
}

fn draw_bar(img: &mut RgbaImage, x: usize, y: usize, bar: &Bar, hue: BarHue) {
    let letter = Lettering::new(20.0, Weight::Regular, SLATE_400);
    let letter_x = x + BAR_LETTER_W - letter.width(&bar.letter).min(BAR_LETTER_W);
    draw_in_line(
        img,
        &letter,
        letter_x,
        y as f32,
        BAR_ROW_H as f32,
        &bar.letter,
    );

    let track_x = x + BAR_LETTER_W + BAR_LETTER_GAP;
    let track_y = y + (BAR_ROW_H - BAR_TRACK_H) / 2;
    fill_round_rect(
        img,
        track_x,
        track_y,
        BAR_TRACK_W,
        BAR_TRACK_H,
        2.0,
        SLATE_700,
    );
    let inner_w = BAR_TRACK_W - 2;
    fill_rect(
        img,
        track_x + 1,
        track_y + 1,
        inner_w,
        BAR_TRACK_H - 2,
        SLATE_800,
    );

    let track_max = bar.ceiling * if bar.soft { SOFT_OVERFLOW } else { 1.0 };
    let fill = (bar.value / track_max).clamp(0.0, 1.0);
    let at_cap = bar.value >= bar.ceiling;
    let colour = if at_cap { hue.capped } else { hue.fill };
    let fill_w = (fill * inner_w as f64).round() as usize;
    fill_rect(
        img,
        track_x + 1,
        track_y + 1,
        fill_w,
        BAR_TRACK_H - 2,
        colour,
    );
    if bar.soft {
        let tick_x = track_x + 1 + (inner_w as f64 / SOFT_OVERFLOW).round() as usize;
        fill_rect(img, tick_x, track_y + 1, 2, BAR_TRACK_H - 2, SLATE_200);
    }
}

/// HP, Regeneration, Recharge and Net End, each as a label, a value and its colour. Formatted
/// shorter than the dashboard where the dashboard's form overruns a tile: Regeneration as its
/// per-second rate, Recharge to one decimal. A stat the registry does not hold is left out
/// rather than shown as a made-up zero.
fn headline_tiles(totals: &CalculatedTotals) -> Vec<(&'static str, String, Rgba<u8>)> {
    let read = |id: &str| stat_registry::by_id(id).map(|def| (def.read)(totals));
    let mut tiles = Vec::new();
    if let Some(hp) = read("health") {
        tiles.push(("HP", format!("{}", hp.floor()), GREEN_400));
    }
    if stat_registry::by_id("regeneration").is_some() {
        let per_second = stat_registry::regen_per_second(totals);
        tiles.push(("REGN", format!("{per_second:.2}/s"), GREEN_400));
    }
    if let Some(recharge) = read("recharge") {
        tiles.push(("RECH", format!("{:.1}%", 100.0 + recharge), SLATE_200));
    }
    if let Some(net) = read("netend") {
        let colour = if net < 0.0 { RED_400 } else { EMERALD_300 };
        tiles.push(("NET END", format!("{net:+.2}/s"), colour));
    }
    tiles
}

fn draw_tile(img: &mut RgbaImage, x: usize, y: usize, label: &str, value: &str, color: Rgba<u8>) {
    fill_round_rect(img, x, y, TILE_W, TILE_H, 12.0, SLATE_700);
    let face = mix(SLATE_800, GRAY_950, 0.7);
    fill_round_rect(img, x + 1, y + 1, TILE_W - 2, TILE_H - 2, 11.0, face);
    let room = TILE_W - 2 - 24;
    let label_top = (y + 1 + TILE_PAD_Y) as f32;
    let label_lettering = Lettering::new(22.0, Weight::Regular, SLATE_400);
    draw_centered(
        img,
        &label_lettering,
        x,
        TILE_W,
        label_top,
        TILE_LABEL_LINE,
        label,
        room,
    );
    // A wide value shrinks to fit rather than being cut: a number missing its last digits is a
    // different number.
    let value_lettering = (24..=38)
        .rev()
        .map(|size| Lettering::new(size as f32, Weight::Bold, color))
        .find(|lettering| lettering.width(value) <= room)
        .unwrap_or_else(|| Lettering::new(24.0, Weight::Bold, color));
    let value_top = label_top + TILE_LABEL_LINE;
    draw_centered(
        img,
        &value_lettering,
        x,
        TILE_W,
        value_top,
        TILE_VALUE_LINE,
        value,
        room,
    );
}

/// What every power row on the card shares.
struct Row<'a> {
    build: &'a CharacterState,
    database: &'a Db,
    art: &'a PowerArt,
}

/// The build's picks in the order they were taken, eight to a column, columns spread across the
/// card. Powers the game granted by itself (level 0) are not picks, so a full build is exactly
/// three columns of eight. Rows that would run below `bottom` are left off.
fn draw_power_columns(img: &mut RgbaImage, top: usize, bottom: usize, row: &Row) {
    let mut powers: Vec<&SelectedPower> = row
        .build
        .all_selected()
        .filter(|power| power.inherent_category.is_none() && power.level > 0)
        .collect();
    powers.sort_by_key(|power| power.level);
    let columns: Vec<&[&SelectedPower]> = powers.chunks(POWERS_PER_COLUMN).collect();
    let widths: Vec<usize> = columns
        .iter()
        .map(|column| {
            column
                .iter()
                .map(|power| power_row_width(power))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let spare = CONTENT_W.saturating_sub(widths.iter().sum());
    let between = match columns.len() {
        0 | 1 => 0,
        n => spare / (n - 1),
    };
    let mut x = PAD;
    for (column, width) in columns.iter().zip(&widths) {
        for (i, power) in column.iter().enumerate() {
            let y = top + i * (POWER_ICON + POWER_ROW_GAP);
            if y + POWER_ICON > bottom {
                break;
            }
            draw_power_row(img, x, y, power, row);
        }
        x += width + between;
    }
}

/// The slotted inherents in one row after their label, as many as fit across the card.
fn draw_inherent_row(
    img: &mut RgbaImage,
    top: usize,
    inherents: &[&SelectedPower],
    build: &CharacterState,
    database: &Db,
    art: &PowerArt,
) {
    let label = Lettering::new(18.0, Weight::Regular, SLATE_500);
    let label_w = draw_in_line(img, &label, PAD, top as f32, POWER_ICON as f32, "INHERENT");
    let row = Row {
        build,
        database,
        art,
    };
    let mut x = PAD + label_w + INHERENT_GAP;
    for power in inherents {
        let width = power_row_width(power);
        if x + width > CONTENT_RIGHT {
            break;
        }
        draw_power_row(img, x, top, power, &row);
        x += width + INHERENT_GAP;
    }
}

fn power_row_width(power: &SelectedPower) -> usize {
    match power.slots.len() {
        0 => POWER_ICON,
        n => POWER_ICON + ICON_TO_SLOTS + n * SLOT_ICON + (n - 1) * SLOT_GAP,
    }
}

/// A power's icon, then each slot: the enhancement's art under its frame, or an empty ring so a
/// power with slots to spare does not read the same as a full one.
fn draw_power_row(img: &mut RgbaImage, x: usize, y: usize, power: &SelectedPower, row: &Row) {
    let icon = crate::view::power_view::resolve_power_def(
        row.database,
        &power.powerset,
        &power.internal_name,
    )
    .and_then(|def| def.extra.get("icon"))
    .and_then(|value| value.as_str())
    .and_then(|name| row.art.get(Some(name)));
    match icon {
        Some(source) => draw_icon(img, source, x, y, POWER_ICON),
        None => {
            fill_round_rect(img, x, y, POWER_ICON, POWER_ICON, 8.0, SLATE_700);
            fill_round_rect(
                img,
                x + 1,
                y + 1,
                POWER_ICON - 2,
                POWER_ICON - 2,
                7.0,
                SLATE_800,
            );
        }
    }
    let slots_x = x + POWER_ICON + ICON_TO_SLOTS;
    let slot_y = y + (POWER_ICON - SLOT_ICON) / 2;
    for (i, slot) in power.slots.iter().enumerate() {
        let slot_x = slots_x + i * (SLOT_ICON + SLOT_GAP);
        draw_slot(img, slot_x, slot_y, slot.as_ref(), row);
    }
}

fn draw_slot(img: &mut RgbaImage, x: usize, y: usize, slot: Option<&Enhancement>, row: &Row) {
    let radius = SLOT_ICON as f32 / 2.0;
    let Some(enhancement) = slot else {
        fill_round_rect(img, x, y, SLOT_ICON, SLOT_ICON, radius, SLATE_700);
        let inside = mix(SLATE_800, GRAY_950, 0.6);
        fill_round_rect(
            img,
            x + 1,
            y + 1,
            SLOT_ICON - 2,
            SLOT_ICON - 2,
            radius - 1.0,
            inside,
        );
        return;
    };
    let base = crate::panels::powers::slot_icon_url(enhancement, row.database)
        .and_then(|url| row.art.enhancement(&url));
    let Some(base) = base else {
        fill_round_rect(img, x, y, SLOT_ICON, SLOT_ICON, radius, SLATE_700);
        return;
    };
    draw_icon(img, base, x, y, SLOT_ICON);
    let origin = row.build.origin.as_deref();
    let frame = crate::panels::powers::slot_overlay_url(enhancement, row.database, origin);
    if let Some(frame) = row.art.enhancement(&frame) {
        draw_icon(img, frame, x, y, SLOT_ICON);
    }
}

fn is_slotted(power: &SelectedPower) -> bool {
    power.slots.iter().any(Option::is_some)
}

/// Draw `source` at `size` square, resampled smoothly: the art is 32px and is drawn larger here,
/// where copying the nearest pixel would leave it blocky.
fn draw_icon(img: &mut RgbaImage, source: &RgbaImage, x: usize, y: usize, size: usize) {
    let resized = imageops::resize(
        source,
        size as u32,
        size as u32,
        imageops::FilterType::CatmullRom,
    );
    draw_image_scaled(img, &resized, x, y, size, size);
}

/// Draw `text` centred vertically in a line `line_h` tall starting at `line_top`, as CSS places
/// text in its line box. Returns the width drawn.
fn draw_in_line(
    img: &mut RgbaImage,
    lettering: &Lettering,
    x: usize,
    line_top: f32,
    line_h: f32,
    text: &str,
) -> usize {
    let top = line_top + (line_h - lettering.height() as f32) / 2.0;
    lettering.draw(img, x, top.round().max(0.0) as usize, text)
}

/// [`draw_in_line`], centred across a box `w` wide at `x`, cut to `room`.
#[allow(clippy::too_many_arguments)]
fn draw_centered(
    img: &mut RgbaImage,
    lettering: &Lettering,
    x: usize,
    w: usize,
    line_top: f32,
    line_h: f32,
    text: &str,
    room: usize,
) {
    let shown = lettering.fit(text, room);
    let text_x = x + (w.saturating_sub(lettering.width(&shown))) / 2;
    draw_in_line(img, lettering, text_x, line_top, line_h, &shown);
}

/// A filled rectangle with smoothed round corners.
fn fill_round_rect(
    img: &mut RgbaImage,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    radius: f32,
    color: Rgba<u8>,
) {
    let radius = radius.min(w as f32 / 2.0).min(h as f32 / 2.0).max(0.0);
    let (left, top) = (x as f32 + radius, y as f32 + radius);
    let (right, bottom) = ((x + w) as f32 - radius, (y + h) as f32 - radius);
    for py in y..y + h {
        for px in x..x + w {
            let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
            let dx = cx - cx.clamp(left, right);
            let dy = cy - cy.clamp(top, bottom);
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = if distance == 0.0 {
                1.0
            } else {
                (radius - distance + 0.5).clamp(0.0, 1.0)
            };
            export_text::blend(img, px as i64, py as i64, color, coverage);
        }
    }
}

/// `top` laid over `under` at `alpha`, as one opaque colour.
fn mix(top: Rgba<u8>, under: Rgba<u8>, alpha: f32) -> Rgba<u8> {
    let channel =
        |i: usize| (f32::from(top[i]) * alpha + f32::from(under[i]) * (1.0 - alpha)).round() as u8;
    Rgba([channel(0), channel(1), channel(2), 255])
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{DatasetId, PowerDatabase};
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn database() -> Db {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contract/homecoming/bundle.json.gz");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        Db(Arc::new(
            PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads"),
        ))
    }

    /// A build file imported as the planner imports one.
    fn import(path: &Path, database: &Db) -> CharacterState {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let mut build = crate::build_file::decode_import(&text, &database.0, DatasetId::Homecoming)
            .expect("build imports")
            .build;
        crate::inherents::sync(&mut build, &database.0);
        crate::granted_powers::sync(&mut build, &database.0);
        build
    }

    /// The art off the native path, which reads embedded files and so never waits.
    fn art(build: &CharacterState, database: &Db) -> PowerArt {
        let mut future = std::pin::pin!(crate::power_art::load_for(build, database));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(art) => art,
            std::task::Poll::Pending => panic!("native art loading waited on something"),
        }
    }

    fn card(path: &Path) -> Vec<u8> {
        let database = database();
        let build = import(path, &database);
        let totals = coh_math::recalculate(&build, &database.0);
        render_preview_png(&build, &database, &totals, &art(&build, &database)).expect("renders")
    }

    fn maiden_fury() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/buildsave/homecoming/maidenfury.txt")
    }

    #[test]
    fn the_card_is_the_size_the_server_accepts() {
        let png = card(&maiden_fury());
        let decoded = image::load_from_memory(&png).expect("a PNG");
        assert_eq!(
            (decoded.width() as usize, decoded.height() as usize),
            (PREVIEW_CARD_WIDTH, PREVIEW_CARD_HEIGHT)
        );
    }

    /// Writes the card to look at: `PREVIEW_BUILD=<build file> cargo test -p app --features
    /// desktop preview_card_sample -- --ignored`, into `target/preview-card.png` (or
    /// `PREVIEW_OUT`). With no `PREVIEW_BUILD` it draws the Maiden Fury fixture.
    #[test]
    #[ignore = "writes a picture to look at; run with --ignored"]
    fn preview_card_sample() {
        let build = std::env::var_os("PREVIEW_BUILD").map_or_else(maiden_fury, PathBuf::from);
        let out = std::env::var_os("PREVIEW_OUT").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/preview-card.png"),
            PathBuf::from,
        );
        std::fs::write(&out, card(&build)).expect("the card writes");
    }
}
