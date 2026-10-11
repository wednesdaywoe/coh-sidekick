//! A negative `Res`-face row on a debuffable stat lowers the TARGET's resistance to that debuff:
//! Sonic Thrust's `Defense All -1 aspect Res to Target` makes the foe easier to −Def. Read through
//! `abs`, it used to land as the caster's own Debuff Resistance, both in the Info panel (under
//! Buffs) and in the totals whenever the power was switched on.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::effect_registry::EffectCategory;
use std::path::PathBuf;

/// The forks that ship Sonic Melee.
const DATASETS: [DatasetId; 2] = [DatasetId::Homecoming, DatasetId::Brainstorm];

const SET: &str = "brute/sonic-melee";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn build(dataset: DatasetId, db: &PowerDatabase) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("brute".into());
    state.primary.id = Some(SET.into());
    assert!(db.resolve_power(SET, "Sonic_Thrust").is_some());
    let mut thrust = SelectedPower::picked("Sonic_Thrust", SET, 1);
    thrust.is_active = true;
    state.primary.powers = vec![thrust];
    coh_data::sync_granted_powers(&mut state, db);
    state
}

#[test]
fn sonic_thrusts_foe_debuff_resistance_is_a_debuff() {
    for dataset in DATASETS {
        let db = load(dataset);
        let totals = coh_math::recalculate(&build(dataset, &db), &db);

        let stats = &totals.stats;
        assert_eq!(
            stats.debuff_resist_defense, 0.0,
            "{dataset:?}: the foe's −DDR counted as the caster's"
        );

        let thrust = totals
            .power_projection
            .iter()
            .find(|p| p.power_internal_name == "Sonic_Thrust")
            .expect("Sonic Thrust projected");
        let rows = &thrust.granted_magnitudes;
        assert!(
            rows.iter().all(|r| r.effect_key != "debuffResistance"),
            "{dataset:?}: a foe −DDR row still reads as the caster's Debuff Res"
        );
        let foe: Vec<_> = rows
            .iter()
            .filter(|r| r.effect_key == "debuffResistanceDebuff")
            .collect();
        assert!(!foe.is_empty(), "{dataset:?}: Sonic Thrust shows no −DDR");
        assert!(
            foe.iter().all(|r| r.category == EffectCategory::Debuff),
            "{dataset:?}: −DDR is not filed as a debuff"
        );
    }
}
