//! Attune switches on the Sonic Melee bonuses, and each one needs three things the data spreads
//! across two powers: the caster running Attune (`kSonicAtuneSelf Source.Mode?`), the target
//! carrying Attune's mark (`kSonicAtuneFoe target.mode?`), and the `SMAttune` chance mod that
//! takes the bonus row from its exported `chance: 0` to certain.
//!
//! Before this, all three were missing. Attune sets its modes one level down, which the mode
//! collector never read; nothing answered a target-mode reader; and the chance-mod detector
//! declined the family because the gate already named Attune, which left every bonus row
//! dormant. Graded through `recalculate` with the toggle itself switched, so a fix to any one
//! layer alone still fails here.
//!
//! The two forks this file named no longer ship the same mechanic, so they are graded apart.
//! The 2026-11-03 Brainstorm beta took Sonic Thrust's side of it out: the `AttuneBonus` group
//! that granted the modes — two `Constant Set_Mode` rows, 26 s on the target and 24.25 s on the
//! caster — is gone from the bin, and the `SMAttune` group that spent them is now an
//! `Execute_Power` of `Redirects.Sonic_Melee.Attune` whose authored `requires_expression` is
//! EMPTY where September spelled the double mark out. `redirects/sonic_melee/sonic_thrust.json`
//! was deleted by the same patch. The mark itself survives on Attune, which now carries
//! `kSonicAtuneFoe` and `kSonicAtune1Foe` rows of its own.
//!
//! So Homecoming pins the bonus and Brainstorm pins its removal: a later re-export that brings
//! the caster-side bonus back, or silently drops Sonic Thrust while pretending the mechanic is
//! still absent, fails here rather than going unnoticed.
//!
//! The `SMAttune` tag is still on Brainstorm's group. That is not the mechanic and is not
//! asserted: PARSE6-3 records that counting tags to decide whether a fork HAS a mechanic is the
//! mistake, because the tag outlives the gate more often than not. Every test below counts gates.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::damage::DamageApplication;
use std::path::PathBuf;

/// The forks that still ship Sonic Thrust's own Attune bonus.
// dataset-absent: brainstorm, rebirth, thunderspy — brainstorm's 2026-11-03 beta moved the bonus
// off Sonic Thrust, which the test below pins; rebirth and thunderspy ship no Sonic Melee.
const DATASETS: &[DatasetId] = &[DatasetId::Homecoming];

/// The fork the 2026-11-03 beta moved the bonus off.
// dataset-absent: homecoming, rebirth, thunderspy — homecoming still ships the bonus and is
// graded by the test above; rebirth and thunderspy ship no Sonic Melee.
const REMOVED_ON: DatasetId = DatasetId::Brainstorm;

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

/// Sonic Thrust as one build sees it: the whole hit, and everything about it that names the
/// target's Attune mark. The mark is read off the GATE — the bonus row's authored condition —
/// rather than off the group's tags, which a fork keeps printing after the gate is gone.
#[derive(Debug)]
struct ThrustHit {
    /// Every component that lands, summed as the projection sums it.
    damage: f64,
    /// Mark-gated components that actually apply, at their total base.
    live_bonus: f64,
    /// Mark-gated components, whatever their application — `Dormant` ones included.
    gated_rows: usize,
    /// Mark-gated rows this context could not settle.
    unresolved_rows: usize,
}

fn read_thrust(dataset: DatasetId, db: &PowerDatabase, attune_on: bool) -> ThrustHit {
    let totals = coh_math::recalculate(&build(dataset, db, attune_on), db);
    let thrust = totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Sonic_Thrust")
        .expect("Sonic Thrust projected");
    let reads_mark = |gate: &str| gate.contains("kSonicAtuneFoe");
    ThrustHit {
        damage: thrust.damage.base,
        live_bonus: thrust
            .damage
            .components
            .iter()
            .filter(|c| c.gate.as_deref().is_some_and(reads_mark))
            .filter(|c| !matches!(c.application, DamageApplication::Dormant))
            .map(|c| c.total.base)
            .sum(),
        gated_rows: thrust
            .damage
            .components
            .iter()
            .filter(|c| c.gate.as_deref().is_some_and(reads_mark))
            .count(),
        unresolved_rows: thrust
            .damage
            .unresolved
            .iter()
            .filter(|row| reads_mark(&row.gate))
            .count(),
    }
}

#[test]
fn attune_switches_on_sonic_thrusts_bonus() {
    for &dataset in DATASETS {
        let db = load(dataset);
        let off = read_thrust(dataset, &db, false);
        assert_eq!(
            off.live_bonus, 0.0,
            "{dataset:?}: Attune off, the bonus still lands"
        );
        assert_eq!(
            off.unresolved_rows, 0,
            "{dataset:?}: Attune off, the bonus is still unresolved"
        );

        let on = read_thrust(dataset, &db, true);
        assert!(
            on.live_bonus > 0.0,
            "{dataset:?}: Attune on, Sonic Thrust deals no bonus"
        );
        assert_eq!(
            on.unresolved_rows, 0,
            "{dataset:?}: Attune on, the bonus is left unresolved"
        );
    }
}

/// The 2026-11-03 removal, pinned. Sonic Thrust still resolves and still deals its own hit — so
/// a green here cannot be the power quietly leaving the bin — and switching Attune moves nothing,
/// because no row of this power asks whether the target is Attuned.
#[test]
fn sonic_thrust_keeps_no_attune_bonus_on_brainstorm() {
    let db = load(REMOVED_ON);
    let off = read_thrust(REMOVED_ON, &db, false);
    let on = read_thrust(REMOVED_ON, &db, true);

    assert!(
        off.damage > 0.0 && on.damage > 0.0,
        "{REMOVED_ON:?}: Sonic Thrust deals nothing at all ({} / {}), which is the power gone \
         rather than the bonus gone",
        off.damage,
        on.damage,
    );
    assert_eq!(
        on.damage, off.damage,
        "{REMOVED_ON:?}: Attune still moves Sonic Thrust ({off:?} -> {on:?}), so the Nov-3 \
         removal this pins has come back - re-read the beta's Sonic Thrust before changing this"
    );
    assert_eq!(
        on.gated_rows + off.gated_rows + on.unresolved_rows + off.unresolved_rows,
        0,
        "{REMOVED_ON:?}: a Sonic Thrust row names the target's Attune mark, so the bonus is back \
         in some form even if this build does not spend it"
    );
}
