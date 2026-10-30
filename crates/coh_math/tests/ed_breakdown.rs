//! The per-aspect ED breakdown agrees with the enhancement the totals use for a power, Alpha
//! included, and reads the stages of Enhancement Diversification the way a player sees them:
//! full value, then less and less for each further damage IO.

use coh_data::{
    CharacterState, DatasetId, Enhancement, IncarnateSlot, Level, PowerDatabase, SelectedPower,
};
use coh_math::ed_breakdown::AspectBreakdown;
use std::path::PathBuf;

const SET: &str = "blaster/fire-blast";
const POWER: &str = "Fire_Blast";

fn load() -> PowerDatabase {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/homecoming/bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
}

fn io(stat: &str) -> Option<Enhancement> {
    Some(Enhancement::generic_io(stat, Level::from_i64(50), 0))
}

/// A level-50 Blaster holding Fire Blast with these slots, and this Alpha if any.
fn build(slots: Vec<Option<Enhancement>>, alpha: Option<&str>) -> CharacterState {
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.level = 50;
    state.archetype.id = Some("blaster".into());
    state.primary.id = Some(SET.into());
    let mut power = SelectedPower::picked(POWER, SET, 1);
    power.slots = slots;
    state.primary.powers = vec![power];
    state.incarnates.alpha = alpha.map(|name| IncarnateSlot {
        power_name: name.into(),
        active: true,
    });
    state
}

fn projected(state: &CharacterState, db: &PowerDatabase) -> coh_math::projection::PowerProjection {
    coh_math::recalculate(state, db)
        .power_projection
        .into_iter()
        .find(|p| p.power_internal_name == POWER)
        .expect("Fire Blast projected")
}

fn damage(rows: &[AspectBreakdown]) -> &AspectBreakdown {
    rows.iter()
        .find(|row| row.aspect == "damage")
        .expect("a damage row")
}

/// What counts, per aspect, is exactly the enhancement the build's numbers use for the power,
/// with and without an Alpha that splits across ED.
#[test]
fn what_counts_is_what_the_totals_use() {
    let db = load();
    let slots = vec![
        io("Damage"),
        io("Damage"),
        io("Damage"),
        io("Accuracy"),
        io("Recharge"),
        io("EnduranceReduction"),
    ];
    for alpha in [None, Some("Musculature_Core_Paragon")] {
        let state = build(slots.clone(), alpha);
        let projection = projected(&state, &db);
        assert_eq!(projection.enhancement_breakdown.len(), 4, "{alpha:?}");
        for row in &projection.enhancement_breakdown {
            assert_eq!(
                Some(&row.counted),
                projection.enhancement_bonuses.get(row.aspect),
                "{alpha:?} {}",
                row.aspect
            );
        }
        // The same per-power enhancement the apply pass feeds the totals from.
        let power = db.resolve_power(SET, POWER).expect("power");
        let alpha_in = coh_math::incarnates::alpha_enhancement(
            &state.incarnates,
            state.combat.exemplar_level,
            &db,
        );
        let direct = coh_math::apply::power_enhancement(
            &slots,
            power,
            &alpha_in,
            50,
            &state.combat,
            &db,
            &mut Vec::new(),
        );
        for row in &projection.enhancement_breakdown {
            assert_eq!(
                row.counted,
                direct.get(row.aspect),
                "{alpha:?} {}",
                row.aspect
            );
        }
    }

    // Musculature's damage splits: 15% joins the slotted sum before ED, 30% lands after it.
    let with = projected(&build(slots.clone(), Some("Musculature_Core_Paragon")), &db);
    let without = projected(&build(slots, None), &db);
    let (with, without) = (
        damage(&with.enhancement_breakdown),
        damage(&without.enhancement_breakdown),
    );
    assert!((with.alpha - 0.15).abs() < 1e-9, "{with:?}");
    assert_eq!(with.slotted, without.slotted, "Alpha is not slotted");
    assert!(
        with.counted > without.counted + 0.30,
        "{with:?} vs {without:?}"
    );
}

/// One damage IO counts in full; each further one reaches the same or a later stage, and ED
/// takes exactly the gap between what is slotted and what counts.
#[test]
fn damage_ios_walk_through_the_stages() {
    let db = load();
    let at = |count: usize| {
        let rows = projected(&build(vec![io("Damage"); count], None), &db).enhancement_breakdown;
        damage(&rows).clone()
    };
    let one = at(1);
    assert_eq!(one.stage, 0);
    assert_eq!(
        one.counted, one.slotted,
        "below the first threshold everything counts"
    );
    assert_eq!(one.lost, 0.0, "nothing lost below the first threshold");
    // Two level-50 damage IOs (84.8%) already pass the first threshold (70%), so the second
    // counts for a little less than the first.
    let two = at(2);
    assert_eq!(two.stage, 1, "{two:?}");
    let second = two.counted - one.counted;
    assert!(
        second < one.counted && second > 0.9 * one.counted,
        "{one:?} {two:?}"
    );

    let stages: Vec<u8> = (1..=6).map(|count| at(count).stage).collect();
    assert!(
        stages.windows(2).all(|pair| pair[0] <= pair[1]),
        "{stages:?}"
    );
    let six = at(6);
    assert_eq!(six.stage, 3, "{six:?}");
    assert!(six.counted < six.slotted);
    // A seventh adds almost nothing.
    let seventh = at(7).counted - six.counted;
    assert!(
        seventh < one.counted * 0.2,
        "a seventh adds {seventh} against {} for the first",
        one.counted
    );
    // Without an Alpha, what ED takes is exactly the gap between slotted and what counts.
    for count in 1..=6 {
        let row = at(count);
        assert!(
            (row.lost - (row.slotted - row.counted)).abs() < 1e-9,
            "{count}: {row:?}"
        );
    }
}

/// Nothing slotted, nothing to break down.
#[test]
fn an_unslotted_power_has_no_breakdown() {
    let db = load();
    assert!(projected(&build(vec![None], None), &db)
        .enhancement_breakdown
        .is_empty());
}
