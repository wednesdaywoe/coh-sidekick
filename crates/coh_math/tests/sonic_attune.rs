//! Attune switches on the Sonic Melee bonuses, and each one needs three things the data spreads
//! across two powers: the caster running Attune (`kSonicAtuneSelf Source.Mode?`), the target
//! carrying Attune's mark (`kSonicAtuneFoe target.mode?`), and the `SMAttune` chance mod that
//! takes the bonus row from its exported `chance: 0` to certain.
//!
//! Before this, all three were missing. Attune sets its modes one level down, which the mode
//! collector never read; nothing answered a target-mode reader; and the chance-mod detector
//! declined the family because the gate already named Attune, which left every bonus row dormant.
//! Graded through `recalculate` with the toggle itself switched, so a fix to any one layer alone
//! still fails here.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::damage::DamageApplication;
use std::path::PathBuf;

/// The forks that ship Sonic Melee.
// dataset-absent: rebirth, thunderspy — neither ships Sonic Melee.
const DATASETS: &[DatasetId] = &[DatasetId::Homecoming, DatasetId::Brainstorm];

const SET: &str = "brute/sonic-melee";

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// A level-50 Brute holding Sonic Thrust and Attune, with Attune switched on or off.
fn build(dataset: DatasetId, db: &PowerDatabase, attune_on: bool) -> CharacterState {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("brute".into());
    state.primary.id = Some(SET.into());
    state.primary.powers = ["Sonic_Thrust", "Attune"]
        .into_iter()
        .map(|pick| {
            assert!(
                db.resolve_power(SET, pick).is_some(),
                "{dataset:?}: no {pick}"
            );
            let mut power = SelectedPower::picked(pick, SET, 1);
            power.is_active = pick != "Attune" || attune_on;
            power
        })
        .collect();
    coh_data::sync_granted_powers(&mut state, db);
    state
}

/// Sonic Thrust's live Attune bonus: the base damage of every component whose gate reads the
/// target's Attune mark and that is not dormant, plus how many unresolved lines name the mark.
fn attune_bonus(state: &CharacterState, db: &PowerDatabase) -> (f64, usize) {
    let totals = coh_math::recalculate(state, db);
    let thrust = totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Sonic_Thrust")
        .expect("Sonic Thrust projected");
    let reads_mark = |gate: &str| gate.contains("kSonicAtuneFoe");
    let live = thrust
        .damage
        .components
        .iter()
        .filter(|c| c.gate.as_deref().is_some_and(reads_mark))
        .filter(|c| !matches!(c.application, DamageApplication::Dormant))
        .map(|c| c.total.base)
        .sum();
    let unresolved = thrust
        .damage
        .unresolved
        .iter()
        .filter(|row| reads_mark(&row.gate))
        .count();
    (live, unresolved)
}

#[test]
fn attune_switches_on_sonic_thrusts_bonus() {
    for &dataset in DATASETS {
        let db = load(dataset);
        let (off, off_unresolved) = attune_bonus(&build(dataset, &db, false), &db);
        assert_eq!(off, 0.0, "{dataset:?}: Attune off, the bonus still lands");
        assert_eq!(
            off_unresolved, 0,
            "{dataset:?}: Attune off, the bonus is still unresolved"
        );

        let (on, on_unresolved) = attune_bonus(&build(dataset, &db, true), &db);
        assert!(
            on > 0.0,
            "{dataset:?}: Attune on, Sonic Thrust deals no bonus"
        );
        assert_eq!(
            on_unresolved, 0,
            "{dataset:?}: Attune on, the bonus is left unresolved"
        );
    }
}
