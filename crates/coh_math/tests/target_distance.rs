//! How far the target is, read per power (`coh_math::distance`).
//!
//! Before this every `distance` gate was Indeterminate, so Attune's whole payload — its damage
//! over time and its debuffs, all held to "within 40 ft" — read as nothing, and Savage Leap's
//! Blood Frenzy grant (one, two or three by the band it is used from) banked nothing in an
//! attack chain. Two directions each: the reach settles what it can and only that, and a chosen
//! band settles the rest.

use coh_data::{CharacterState, DatasetId, PowerDatabase, SelectedPower};
use coh_math::chain_walk::{EdgeEnv, WalkState};
use coh_math::distance::{bands, bounds, DistanceBand};
use coh_math::expr::TargetIdentity;
use std::path::PathBuf;

fn load() -> PowerDatabase {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/homecoming/bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("load homecoming")
}

fn build(db: &PowerDatabase, archetype: &str, set: &str, pick: &str) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = 50;
    state.archetype.id = Some(archetype.into());
    state.primary.id = Some(set.into());
    let mut power = SelectedPower::picked(pick, set, 1);
    power.is_active = true;
    state.primary.powers = vec![power];
    coh_data::sync_granted_powers(&mut state, db);
    state
}

#[test]
fn attunes_payload_counts_within_its_reach() {
    let db = load();
    let totals = coh_math::recalculate(&build(&db, "brute", "brute/sonic-melee", "Attune"), &db);
    let attune = totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == "Attune")
        .expect("Attune projected");
    assert!(
        attune.damage.base > 0.0,
        "Attune deals no damage: {:?}",
        attune.damage
    );
    assert!(
        attune.damage.unresolved.is_empty(),
        "Attune's damage is still waiting on something: {:?}",
        attune.damage.unresolved
    );
    assert!(
        attune
            .granted_magnitudes
            .iter()
            .any(|row| row.effect_key == "resistanceDebuff"),
        "Attune's -Resistance is missing from its rows"
    );
    // Its 20 ft reach answers every gate it has, so it offers no distance choice.
    let def = db.resolve_power("brute/sonic-melee", "Attune").unwrap();
    assert!(bands(def).is_empty());
}

/// The bands a held power offers, as its projection states them.
fn offered(db: &PowerDatabase, archetype: &str, set: &str, pick: &str) -> Vec<DistanceBand> {
    let totals = coh_math::recalculate(&build(db, archetype, set, pick), db);
    totals
        .power_projection
        .iter()
        .find(|p| p.power_internal_name == pick)
        .unwrap_or_else(|| panic!("{pick} projected"))
        .distance_bands
        .clone()
}

/// A power offers the bands its own gates cut, and only where picking one settles something.
#[test]
fn a_power_offers_the_bands_its_own_gates_cut() {
    let db = load();
    let band = |lo, hi| DistanceBand { lo, hi };
    assert_eq!(
        offered(&db, "brute", "brute/savage-melee", "Savage_Leap"),
        vec![
            band(0.0, 7.0),
            band(7.0, 20.0),
            band(20.0, 40.0),
            band(40.0, 70.0)
        ]
    );
    // Telekinesis cuts three bands, but every row they settle either waits on the target being
    // `Immobilized` (its repel) or has no row to show (its Intangible): a choice would move
    // nothing, so none is offered.
    let telekinesis = db
        .resolve_power("controller/mind-control", "Telekinesis")
        .unwrap();
    assert_eq!(bands(telekinesis).len(), 3);
    assert!(offered(&db, "controller", "controller/mind-control", "Telekinesis").is_empty());
}

/// Blood Frenzy banked by one Savage Leap at `distance`, and how many grants were left unread.
fn leap_banks(db: &PowerDatabase, distance: Option<f64>) -> (f64, usize) {
    let leap = db
        .resolve_power("brute/savage-melee", "Savage_Leap")
        .unwrap();
    let env = EdgeEnv {
        base_owned: Default::default(),
        source_modes: Default::default(),
        in_combat: true,
        hidden: None,
        target: Some(TargetIdentity {
            archetype_class: None,
            entity_type: "critter".into(),
            modes: Default::default(),
            held: false,
        }),
        distance: Some(bounds(leap, distance)),
    };
    let mut walk = WalkState::new(None);
    let outcome = walk
        .apply_cast_edges(leap, 0.0, &env, &mut Vec::new())
        .expect("Savage Leap's grant edges read");
    let banked = walk
        .owned_overlay_at(0.1)
        .iter()
        .filter(|(path, _)| path.contains("blood_frenzy"))
        .map(|(_, count)| count)
        .sum();
    (banked, outcome.indeterminate)
}

#[test]
fn savage_leap_banks_blood_frenzy_by_the_band_it_was_used_from() {
    let db = load();
    // No choice: 0–70 ft straddles every band, so no grant is guessed.
    let (banked, unread) = leap_banks(&db, None);
    assert_eq!(
        banked, 0.0,
        "a leap from nowhere in particular banked Blood Frenzy"
    );
    assert!(unread > 0, "the unchosen bands were not reported as unread");

    for (distance, stacks) in [(10.0, 1.0), (30.0, 2.0), (55.0, 3.0)] {
        let (banked, unread) = leap_banks(&db, Some(distance));
        assert_eq!(banked, stacks, "a leap from {distance} ft");
        assert_eq!(unread, 0, "a leap from {distance} ft left a grant unread");
    }
}

/// Savage Leap's damage scales `1 + clamp(distance − 25, 0, 40) / 40`: flat to 25 ft, double from
/// 65. A chosen band values it over the band — one number near, a range in the band it climbs
/// through — rather than over every distance at once.
#[test]
fn savage_leaps_damage_follows_the_chosen_band() {
    let db = load();
    let hit = |distance: Option<f64>| {
        let mut state = build(&db, "brute", "brute/savage-melee", "Savage_Leap");
        state.primary.powers[0].target_distance = distance;
        let totals = coh_math::recalculate(&state, &db);
        let leap = totals
            .power_projection
            .iter()
            .find(|p| p.power_internal_name == "Savage_Leap")
            .expect("Savage Leap projected");
        (leap.damage.base, leap.damage.high.map(|high| high.base))
    };
    let (any_low, any_high) = hit(None);
    let any_high = any_high.expect("unchosen, the damage spans every distance");
    assert!(
        (any_high / any_low - 2.0).abs() < 1e-6,
        "{any_low}–{any_high}"
    );

    let (near, near_high) = hit(Some(DistanceBand { lo: 0.0, hi: 7.0 }.midpoint()));
    assert!(
        (near - any_low).abs() < 1e-6 && near_high.is_none(),
        "{near} {near_high:?}"
    );

    let (far_low, far_high) = hit(Some(DistanceBand { lo: 40.0, hi: 70.0 }.midpoint()));
    assert!((far_low / any_low - 1.375).abs() < 1e-6, "{far_low}");
    assert!((far_high.expect("40+ climbs") / any_low - 2.0).abs() < 1e-6);
}

/// The chain reads the same choice: a rotation opening on Savage Leap banks Blood Frenzy only
/// once the leap's band is chosen.
#[test]
fn an_attack_chain_reads_the_leaps_band() {
    let db = load();
    let banked = |distance: Option<f64>| {
        let mut state = build(&db, "brute", "brute/savage-melee", "Savage_Leap");
        state.primary.powers[0].target_distance = distance;
        let totals = coh_math::recalculate(&state, &db);
        let inputs = coh_math::chain_build::chain_inputs(&state, &totals, &db, None, false);
        let leap = inputs
            .powers
            .iter()
            .position(|p| p.id.ends_with("Savage_Leap"))
            .expect("Savage Leap is a chain candidate");
        let bounds = inputs.bounds.expect("a Brute has recharge bounds");
        let chain = coh_math::chain_build::schedule_chain(
            &state,
            &db,
            &inputs,
            None,
            &[leap],
            totals.bonuses.recharge,
            bounds,
        );
        chain.notes[0]
            .as_ref()
            .expect("the cast has a note")
            .banked
            .clone()
    };
    assert!(
        banked(None).is_empty(),
        "an unchosen band banked Blood Frenzy"
    );
    let far = banked(Some(DistanceBand { lo: 40.0, hi: 70.0 }.midpoint()));
    assert!(
        far.iter().any(|path| path.contains("blood_frenzy")),
        "a leap from 40+ ft banked nothing: {far:?}"
    );
}
