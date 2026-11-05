//! A debuff-resistance (DDR) row takes its OWN attribute's enhancement, unless the granting
//! template carries `IgnoreStrength`. Two readings of the game, one bit apart.
//!
//! Reading one — a flagged row. Level-50 Tanker, Stone Armor, Granite Armor ON, on the
//! Homecoming test server, 2026-10-24:
//!
//! ```text
//! level-50 Defense IOs slotted   0    1    2    3    4    5    6
//! Defense DDR %               43.25 43.25 43.25 43.25 43.25 43.25 43.25
//! ```
//!
//! Reading two — an unflagged row. The game's own Defense Resistance breakdown on a level-50
//! Super Reflexes character, 2026-11-05, each row labelled `from Self`:
//!
//! ```text
//! Evasion / Focused Fighting / Focused Senses   +27.66%   (engine, unenhanced: 17.30)
//! Agile / Dodge / Lucky                         +13.66 / 13.58 / 13.62%   (engine: 8.65)
//! ```
//!
//! Both are the game being consistent. Granite's DDR template carries `IgnoreStrength`
//! (`flags_raw` 0x430) and the seven Super Reflexes DDR templates do not (0x420; Evasion 0x60),
//! and that single bit is the whole difference. Agile, Dodge and Lucky are identical in the
//! data — same `Defense/All` scale 0.2, same `Melee_Res_Boolean` table, same `boosts_allowed`
//! of `Buff_Defense` alone — yet the game gives all three a different number, which is only
//! possible if each power's own Defense enhancement reaches its own DDR row.
//!
//! Both planner lineages had the multiplier and not the flag: the beta's
//! `debuffResEnhMapping = { defense: 'defense' }` and Mids Reborn's `GBPA_Pass1_EnhancePreED`,
//! which routes `ResEffect`/`ETModifies == Defense` onto `eEnhance.Defense`. Unconditioned,
//! they put Granite at 73.48% where the game holds it at 43.25%. The rebuild's first fix went
//! the other way and removed the multiplier for every type, which held Super Reflexes at 17.30
//! where the game reads 27.66. The flag is the condition both were missing.
//!
//! The flag is the ONLY thing in the data that can make a row unenhanceable, so no reading of
//! `allowedEnhancements` can replace it: a DDR row is the `Res` face of the same attrib its
//! enhanceable buff row uses, and the two share the power's boost list. That is why these tests
//! read a flagged power and an unflagged one side by side rather than trusting a slot list, and
//! why the multiplier is keyed by the resisted attribute — the same attrib-keyed rule mez
//! resistance already uses — rather than hardcoded to defense.
//!
//! Every number here is read below the per-type ceiling, because `finalize` clamps each row to
//! its own (95% for defense from level 47 up) and a clamped row would hide the multiplier. The
//! ceilings themselves are pinned by `debuff_resistance_caps.rs` and the last two tests.

use coh_data::{CharacterState, DatasetId, Enhancement, PowerDatabase, SelectedPower};
use std::path::PathBuf;

/// All four forks ship Tanker Stone Armor with Granite Armor and Tanker Super Reflexes.
const DATASETS: &[DatasetId] = &[
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Brainstorm,
    DatasetId::Thunderspy,
];

const HC: DatasetId = DatasetId::Homecoming;

const SET: &str = "tanker/stone-armor";
const POWER: &str = "Granite_Armor";

/// The set whose DDR powers stack without exclusivity — see [`super_reflexes`].
const SR_SET: &str = "tanker/super-reflexes";
const SR_TOGGLES: &[&str] = &[
    "Focused_Fighting",
    "Focused_Senses",
    "Agile",
    "Dodge",
    "Evasion",
    "Lucky",
    "Elude",
];

/// Granite's Defense DDR, unenhanced: `scale 1.0` on the Tanker's `Melee_Res_Boolean` at 50.
const GRANITE_DDR: f64 = 43.25;

/// Focused Fighting's Defense DDR unenhanced: `scale 0.4` on the same table.
const FOCUSED_FIGHTING_DDR: f64 = 17.3;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// One picked, switched-on power with one level-50 generic IO of each named stat in it.
fn slotted(power: &str, set: &str, pick_level: u8, stats: &[&str]) -> SelectedPower {
    let mut picked = SelectedPower::picked(power, set, pick_level);
    picked.is_active = true;
    picked.slots = stats
        .iter()
        .map(|stat| Some(Enhancement::generic_io(*stat, None, 0)))
        .collect();
    picked
}

/// Level-50 Tanker with Granite Armor ON and `defense_slots` level-50 Defense IOs slotted.
fn build(dataset: DatasetId, defense_slots: usize) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("tanker".into());
    state.primary.id = Some(SET.into());
    state.primary.powers = vec![slotted(POWER, SET, 26, &vec!["Defense"; defense_slots])];
    state
}

fn ddr(dataset: DatasetId, db: &PowerDatabase, defense_slots: usize) -> f64 {
    coh_math::recalculate(&build(dataset, defense_slots), db)
        .stats
        .debuff_resist_defense
}

/// Granite's positional Smashing defense — a row every planner agrees takes the power's
/// Defense enhancement, and which [`granite_positional_defense_still_takes_defense_enhancement`]
/// below pins at 15 -> 25.49.
fn granite_positional_defense(db: &PowerDatabase, defense_slots: usize) -> f64 {
    coh_math::recalculate(&build(HC, defense_slots), db)
        .stats
        .def_smashing
}

/// The factor a level-50 power's Defense enhancement applies at, measured rather than
/// hardcoded: what a positional defense row of a slotted level-50 power grows by. Every
/// unflagged DDR row of such a power must grow by exactly this, and no flagged row at all.
fn defense_enhancement_factor(db: &PowerDatabase, defense_slots: usize) -> f64 {
    granite_positional_defense(db, defense_slots) / granite_positional_defense(db, 0)
}

/// Level-50 Tanker, Super Reflexes primary, ONE DDR toggle on with `slots` Defense IOs in it.
///
/// One toggle rather than the seven-toggle stack because the stack's 121.1% is clamped to the
/// 95% ceiling, and a clamped row cannot show a multiplier at all.
fn one_reflex(power: &str, slots: usize) -> CharacterState {
    let mut state = CharacterState::empty(HC);
    state.level = 50;
    state.archetype.id = Some("tanker".into());
    state.primary.id = Some(SR_SET.into());
    state.primary.powers = vec![slotted(power, SR_SET, 44, &vec!["Defense"; slots])];
    state
}

/// A level-50 Mastermind with Sonic Resonance's Sonic Siphon on, holding one IO of each named
/// stat — the only power on any fork whose resistance to a recharge drain is both unflagged and
/// slottable with Recharge enhancement.
///
/// It is also the clearest proof that the flag is read per row rather than per power NAME: the
/// same Sonic Siphon carries its self-facing DDR rows FLAGGED on the Defender, Controller and
/// Corruptor versions of the set and UNFLAGGED on the Mastermind's, so a Defender slotted to the
/// gills reads 43.25 flat where this build climbs.
fn sonic_siphon(stats: &[&str]) -> CharacterState {
    const SONIC: &str = "mastermind/sonic-resonance";
    let mut state = CharacterState::empty(HC);
    state.level = 50;
    state.archetype.id = Some("mastermind".into());
    state.secondary.id = Some(SONIC.into());
    state.secondary.powers = vec![slotted("Sonic_Siphon", SONIC, 10, stats)];
    state
}

/// A level-50 Brute with Fiery Aura's Consume on — the corpus's endurance-drain resistance,
/// which EnduranceReduction enhancement reaches and nothing else does.
fn consume(stats: &[&str]) -> CharacterState {
    const FIERY: &str = "brute/fiery-aura";
    let mut state = CharacterState::empty(HC);
    state.level = 50;
    state.archetype.id = Some("brute".into());
    state.secondary.id = Some(FIERY.into());
    state.secondary.powers = vec![slotted("Consume", FIERY, 10, stats)];
    state
}

/// The game's flat line on the flagged power: every slot count reads the unenhanced number.
#[test]
fn granite_defense_ddr_ignores_defense_enhancement() {
    for &dataset in DATASETS {
        let db = load(dataset);
        // Tolerance, not equality: the AT table carries the float32 value the export read
        // (0.4325000047683716), so the product is 43.25000047683716 rather than 43.25.
        let unslotted = ddr(dataset, &db, 0);
        assert!(
            (unslotted - GRANITE_DDR).abs() < 1e-4,
            "{dataset:?}: Granite's unslotted DDR reads {unslotted}, expected {GRANITE_DDR}"
        );

        for slots in 1..=6 {
            let enhanced = ddr(dataset, &db, slots);
            assert!(
                (enhanced - GRANITE_DDR).abs() < 1e-4,
                "{dataset:?}: {slots} Defense IO(s) moved Granite's DDR from {GRANITE_DDR} to \
                 {enhanced}; the game holds it flat, because the granting template carries \
                 IgnoreStrength"
            );
        }
    }
}

/// The fix must not read as "defense enhancement does nothing to this power". Granite's
/// positional defense rows are unflagged and still take every point of it. Without this half,
/// a fix that zeroed the whole defense aspect would also pass.
#[test]
fn granite_positional_defense_still_takes_defense_enhancement() {
    for &dataset in DATASETS {
        let db = load(dataset);
        let bare = coh_math::recalculate(&build(dataset, 0), &db)
            .stats
            .def_smashing;
        let slotted = coh_math::recalculate(&build(dataset, 6), &db)
            .stats
            .def_smashing;
        assert!(
            bare > 0.0,
            "{dataset:?}: Granite grants no Smashing defense to begin with"
        );
        assert!(
            slotted > bare * 1.5,
            "{dataset:?}: 6 Defense IOs left Smashing defense at {slotted} (base {bare}) — the \
             positional defense rows are supposed to enhance"
        );
    }
}

/// The unflagged power climbs, and by exactly the factor a positional defense row of the same
/// level and same IO level takes — same attrib, same enhancement, same multiplier.
#[test]
fn super_reflexes_defense_ddr_takes_defense_enhancement() {
    let db = load(HC);
    let unslotted = coh_math::recalculate(&one_reflex("Focused_Fighting", 0), &db)
        .stats
        .debuff_resist_defense;
    assert!(
        (unslotted - FOCUSED_FIGHTING_DDR).abs() < 1e-4,
        "Focused Fighting's unslotted DDR reads {unslotted}, expected {FOCUSED_FIGHTING_DDR}"
    );

    for slots in 1..=6 {
        let shown = coh_math::recalculate(&one_reflex("Focused_Fighting", slots), &db)
            .stats
            .debuff_resist_defense;
        let expected = FOCUSED_FIGHTING_DDR * defense_enhancement_factor(&db, slots);
        assert!(
            shown > unslotted,
            "{slots} Defense IOs in Focused Fighting left its DDR at {shown}; an unflagged row \
             must take the power's own Defense enhancement"
        );
        assert!(
            (shown - expected).abs() < 0.05,
            "{slots} Defense IOs read {shown}, expected {expected} — Focused Fighting's 17.3 \
             grown by the same factor its positional defense row takes"
        );
    }
}

/// The one bit, both ways, in one place: same level, same archetype, same six Defense IOs, same
/// `Melee_Res_Boolean` table — the flagged power does not move and the unflagged power does.
/// Nothing else in the data separates them, so if this pair ever agrees, the flag has stopped
/// being read.
#[test]
fn the_ignore_strength_flag_is_what_divides_the_two_readings() {
    let db = load(HC);
    let granite = ddr(HC, &db, 6);
    let reflexes = coh_math::recalculate(&one_reflex("Focused_Fighting", 6), &db)
        .stats
        .debuff_resist_defense;
    assert!(
        (granite - GRANITE_DDR).abs() < 1e-4,
        "Granite (IgnoreStrength) moved off {GRANITE_DDR} with 6 Defense IOs: {granite}"
    );
    assert!(
        reflexes > FOCUSED_FIGHTING_DDR * 1.3,
        "Focused Fighting (unflagged) with 6 Defense IOs reads {reflexes} against its unslotted \
         {FOCUSED_FIGHTING_DDR}: the multiplier is not being applied"
    );
}

/// The rule is not a defense special case: Consume's resistance to an endurance drain takes
/// EnduranceReduction enhancement, which no defense-only reading would ever look for — and a
/// Defense IO, a category Consume does not even accept, reaches nothing.
#[test]
fn consume_endurance_ddr_takes_endurance_reduction() {
    let db = load(HC);
    let base = coh_math::recalculate(&consume(&[]), &db)
        .stats
        .debuff_resist_endurance;
    let enhanced = coh_math::recalculate(&consume(&["EnduranceReduction"; 3]), &db)
        .stats
        .debuff_resist_endurance;
    assert!(
        base > 0.0,
        "Consume grants no endurance-drain resistance, so this test proves nothing"
    );
    assert!(
        enhanced > base,
        "3 EnduranceReduction IOs left Consume's endurance DDR flat at {base} (reads {enhanced})"
    );
    let wrong = coh_math::recalculate(&consume(&["Defense"; 3]), &db)
        .stats
        .debuff_resist_endurance;
    assert!(
        (wrong - base).abs() < 1e-9,
        "3 Defense IOs moved Consume's endurance DDR {base} -> {wrong}"
    );
}

/// And again on a third attribute, on a power carrying unflagged self-facing rows for eight of
/// them at once: Sonic Siphon accepts Resistance, EnduranceReduction, Range, Recharge and
/// Accuracy, and only its recharge row moves — the multiplier is keyed by the resisted
/// attribute, not applied to every row a slotted power happens to carry.
#[test]
fn sonic_siphon_recharge_ddr_takes_recharge_and_no_other_category() {
    let db = load(HC);
    let read = |stats: &[&str]| {
        let s = coh_math::recalculate(&sonic_siphon(stats), &db).stats;
        (
            s.debuff_resist_recharge,
            s.debuff_resist_defense,
            s.debuff_resist_slow,
            s.debuff_resist_regeneration,
        )
    };
    let (base_rech, base_def, base_slow, base_regen) = read(&[]);
    assert!(
        base_rech > 0.0 && base_def > 0.0 && base_slow > 0.0 && base_regen > 0.0,
        "Sonic Siphon grants no DDR rows to test: {base_rech} / {base_def} / {base_slow} / \
         {base_regen}"
    );

    let (rech, def, slow, regen) = read(&["Recharge"; 3]);
    assert!(
        rech > base_rech
            && (def - base_def).abs() < 1e-9
            && (slow - base_slow).abs() < 1e-9
            && (regen - base_regen).abs() < 1e-9,
        "3 Recharge IOs moved recharge {base_rech}->{rech}, defense {base_def}->{def}, slow \
         {base_slow}->{slow}, regeneration {base_regen}->{regen}; only the recharge row takes it"
    );

    // Accepted categories that name no resisted attribute of this power.
    for stats in [
        &["Resistance"; 3][..],
        &["Accuracy"; 3][..],
        &["Range"; 3][..],
    ] {
        let now = read(stats);
        assert!(
            now == (base_rech, base_def, base_slow, base_regen),
            "{stats:?} IOs moved Sonic Siphon's DDR {base_rech}/{base_def}/{base_slow}/\
             {base_regen} -> {now:?}"
        );
    }
}

/// A resisted attribute with no enhancement category of its own reads unenhanced on every
/// power, flagged or not. Focused Senses carries both halves — its perception row has no aspect
/// to take and its defense row does — so one build with Defense IOs tells them apart.
#[test]
fn a_resisted_attribute_with_no_enhancement_category_stays_flat() {
    let db = load(HC);
    let read = |slots: usize| {
        let s = coh_math::recalculate(&one_reflex("Focused_Senses", slots), &db).stats;
        (s.debuff_resist_perception, s.debuff_resist_defense)
    };
    let (perc_base, def_base) = read(0);
    assert!(
        perc_base > 0.0 && def_base > 0.0,
        "Focused Senses grants no perception/defense DDR: {perc_base} / {def_base}"
    );
    let (perc, def) = read(3);
    assert!(
        def > def_base,
        "3 Defense IOs in Focused Senses left its defense DDR at {def_base}"
    );
    assert!(
        (perc - perc_base).abs() < 1e-9,
        "3 Defense IOs moved Focused Senses' perception DDR {perc_base} -> {perc}; perception has \
         no enhancement category, so nothing may enhance that row"
    );
}

// === The per-type ceilings, which the enhancement above can push a row past ===

/// DDR-CAP-1: the Super Reflexes stack reaches 121.1% raw and the game caps it. At level 50
/// every fork's ceiling is 95: the archetype tables' curve has flattened there.
#[test]
fn the_super_reflexes_stack_clamps_to_95_at_level_50() {
    for &dataset in DATASETS {
        let db = load(dataset);
        let totals = coh_math::recalculate(&super_reflexes(dataset, 50), &db);
        assert_eq!(
            totals.stats.debuff_resist_defense, 95.0,
            "{dataset:?}: the seven-toggle stack should clamp to 95, not read its raw 121.1"
        );
        assert_eq!(
            totals.stats.debuff_resist_cap.defense, 95.0,
            "{dataset:?}: the ceiling travels with the clamped number"
        );
    }
}

/// Below level 47 the ceiling climbs with level on every fork — the archetype tables carry the
/// same curve on all four, 20% at level 1 to 95% at 47. A flat 95 would pass the level-50 test
/// above and get every row here wrong: the stack's raw value (280 × the Tanker's
/// `melee_res_boolean` row) is under 95 until level 40, so a flat ceiling would read it raw.
#[test]
fn below_level_47_every_fork_clamps_to_the_curve() {
    // (level, raw 280 × melee_res_boolean, the curve)
    const ROWS: &[(u8, f64, f64)] = &[
        (1, 52.50, 20.00),
        (10, 65.10, 34.76),
        (25, 86.10, 59.36),
        (40, 107.10, 83.96),
        (46, 115.50, 93.80),
    ];
    for &dataset in DATASETS {
        let db = load(dataset);
        for &(level, raw, curve) in ROWS {
            let totals = coh_math::recalculate(&super_reflexes(dataset, level), &db);
            let shown = totals.stats.debuff_resist_defense;
            assert!(
                (shown - curve).abs() < 1e-4,
                "{dataset:?} at level {level}: shows {shown}, expected the curve's {curve} \
                 (raw {raw})"
            );
            assert!(
                (totals.stats.debuff_resist_cap.defense - curve).abs() < 1e-4,
                "{dataset:?} at level {level}: the ceiling travels with it"
            );
        }
    }
}

/// Level-50 Tanker, Super Reflexes primary with all seven DDR toggles on — the build that
/// reaches 121.1% raw, and the only way a legal Tanker can clear the 95 ceiling. Every DDR
/// set is primary-only, so one set is all the DDR a build can hold; Super Reflexes is the one
/// DDR set whose powers have no toggle exclusivity between them, so all seven stay up at once
/// (Stone Armor's armors are mutually exclusive via `Granite_Mode`, which is why Granite plus
/// Rooted only reaches 60.55). No power pool or epic pool carries a DDR atom on any fork.
fn super_reflexes(dataset: DatasetId, level: u8) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = level;
    state.archetype.id = Some("tanker".into());
    state.primary.id = Some(SR_SET.into());
    state.secondary.id = Some("tanker/super-strength".into());
    state.primary.powers = SR_TOGGLES
        .iter()
        .map(|name| {
            let mut picked = SelectedPower::picked(*name, SR_SET, level);
            picked.is_active = true;
            picked
        })
        .collect();
    state
}
