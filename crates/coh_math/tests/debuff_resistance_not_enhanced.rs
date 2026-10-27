//! Defense enhancement does NOT enhance defense-debuff resistance (DDR-1).
//!
//! Measured on the Homecoming test server, level 50 Tanker, Stone Armor, Granite Armor ON:
//!
//! ```text
//! level-50 Defense IOs slotted   0    1    2    3    4    5    6
//! Defense DDR %               43.25 43.25 43.25 43.25 43.25 43.25 43.25
//! ```
//!
//! Flat. Both planner lineages said the opposite — the beta's
//! `debuffResEnhMapping = { defense: 'defense' }` and Mids Reborn's
//! `GBPA_Pass1_EnhancePreED`, which routes `ResEffect`/`ETModifies == Defense` onto
//! `eEnhance.Defense` — and the rebuild ported the beta. With six Defense IOs the port read
//! 73.48% against the game's 43.25%.
//!
//! The unslotted 43.25 is `scale 1.0 × Tanker melee_res_boolean[50]`, so the table read was
//! already right; only the multiplier was wrong. The data says the same thing once the correct
//! boost token is asked about: a DDR row is `Base_Defense` with `aspect=Resistance`, the token
//! that governs resisting a defense debuff is `Debuff_Defense`, and across all four datasets
//! 1168 powers carry a DDR row, 2373 allow `Debuff_Defense`, and the overlap is zero. Granite
//! allows `Buff_Defense`, which is the token for its positional defense BUFF rows — a different
//! row of the same power. So a power listing "Defense" in `allowedEnhancements` says nothing
//! about whether its DDR row is enhanceable, and this test is what keeps that from being
//! re-derived backwards from the slot list.

use coh_data::{CharacterState, DatasetId, Enhancement, PowerDatabase, SelectedPower};
use std::path::PathBuf;

/// All four forks ship Tanker Stone Armor with Granite Armor.
const DATASETS: &[DatasetId] = &[
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Brainstorm,
    DatasetId::Thunderspy,
];

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

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Level-50 Tanker with Granite Armor ON and `defense_slots` level-50 Defense IOs slotted.
fn build(dataset: DatasetId, defense_slots: usize) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("tanker".into());
    state.primary.id = Some(SET.into());
    let mut granite = SelectedPower::picked(POWER, SET, 26);
    granite.is_active = true;
    granite.slots = (0..defense_slots)
        .map(|_| Some(Enhancement::generic_io("Defense", None, 0)))
        .collect();
    state.primary.powers = vec![granite];
    state
}

fn ddr(dataset: DatasetId, db: &PowerDatabase, defense_slots: usize) -> f64 {
    coh_math::recalculate(&build(dataset, defense_slots), db)
        .stats
        .debuff_resist_defense
}

/// The game's flat line: every slot count reads the unenhanced number.
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
                (enhanced - unslotted).abs() < 1e-9,
                "{dataset:?}: {slots} Defense IO(s) moved Granite's DDR from {unslotted} to \
                 {enhanced}; the game holds it at {GRANITE_DDR} at every slot count"
            );
        }
    }
}

/// The positional defense BUFF rows on the SAME power DO take Defense enhancement, so this is
/// not "defense enhancement does nothing here" — it is specifically the DDR row that is inert.
/// Without this half, a fix that zeroed the whole defense aspect would also pass.
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
            "{dataset:?}: 6 Defense IOs left Smashing defense at {slotted} (base {bare}) — \
             the positional defense rows are supposed to enhance"
        );
    }
}

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
