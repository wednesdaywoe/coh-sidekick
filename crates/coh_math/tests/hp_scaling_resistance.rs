//! Super Reflexes' passives grant damage resistance that rises as health falls: Agile, Dodge and
//! Lucky each state `60 kHitPoints% source> - 0 100 minmax 60 / 0.2 *`, nothing at 60% health and
//! above, 20% at zero. The atom's `scale` is 0 on `Melee_Ones`, so read the ordinary way it is a
//! silent zero and lowering the combat panel's Health % moved nothing.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

const SR_SET: &str = "tanker/super-reflexes";

fn super_reflexes(db: &PowerDatabase, picks: &[&str], health: f64) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = 50;
    state.archetype.id = Some("tanker".into());
    state.primary.id = Some(SR_SET.into());
    state.primary.powers = picks
        .iter()
        .map(|pick| {
            assert!(db.resolve_power(SR_SET, pick).is_some(), "no {pick}");
            let mut power = SelectedPower::picked(*pick, SR_SET, 1);
            power.is_active = true;
            power
        })
        .collect();
    coh_data::sync_granted_powers(&mut state, db);
    state.combat.hit_points_percent = health;
    state
}

#[test]
fn super_reflexes_resistance_rises_as_health_falls() {
    let db = load(DatasetId::Homecoming);
    let picks = ["Agile", "Dodge", "Lucky"];
    let fire = |health: f64| {
        coh_math::recalculate(&super_reflexes(&db, &picks, health), &db)
            .stats
            .res_fire
    };
    let full = fire(100.0);
    assert_eq!(full, fire(60.0), "nothing at 60% health and above");
    // Each passive: (60 − 30) / 60 × 20% = 10%.
    let hurt = fire(30.0) - full;
    assert!((hurt - 30.0).abs() < 1e-6, "three passives at 30% health: {hurt}");
    let floor = fire(0.0) - full;
    assert!((floor - 60.0).abs() < 1e-6, "three passives at 0% health: {floor}");
}

#[test]
fn super_reflexes_scaling_resistance_skips_toxic_and_psionic_in_pve() {
    let db = load(DatasetId::Homecoming);
    let state = |health| super_reflexes(&db, &["Agile"], health);
    let full = coh_math::recalculate(&state(100.0), &db).stats;
    let hurt = coh_math::recalculate(&state(0.0), &db).stats;
    assert_eq!(full.res_toxic, hurt.res_toxic);
    assert_eq!(full.res_psionic, hurt.res_psionic);
    assert!(hurt.res_smashing > full.res_smashing);
}
