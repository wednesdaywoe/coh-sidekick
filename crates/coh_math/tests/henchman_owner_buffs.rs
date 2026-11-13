//! A Mastermind henchman's buff on its owner reaches the owner's totals behind the buff-pet switch.
//!
//! Two henchmen carry one on every fork: the Protector Bot's Force Shield (+Defense) and the Ember
//! Demon's pair of Ember Shields (+Resistance). Force Shield is `caster_stack: Collective`, so two
//! bots are one shield. The owner gets BOTH Ember Shields, and Enchant Demon swaps them for
//! versions the summon's Resistance slotting raises (Homecoming and Brainstorm revoke the base
//! pair; Rebirth and Thunderspy's upgrade grants no new shield).

use coh_data::{CharacterState, DatasetId, Enhancement, PowerDatabase, SelectedPower};
use coh_math::totals::GlobalBonuses;
use std::path::PathBuf;

const ROBOTICS: &str = "mastermind/robotics";
const DEMONS: &str = "mastermind/demon-summoning";
const FORKS: [DatasetId; 4] = [
    DatasetId::Homecoming,
    DatasetId::Rebirth,
    DatasetId::Thunderspy,
    DatasetId::Brainstorm,
];

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

struct Build<'a> {
    set: &'a str,
    summon: &'a str,
    slots: &'a [&'a str],
    also: &'a [&'a str],
    switched_on: bool,
}

fn totals(db: &PowerDatabase, dataset: DatasetId, build: Build<'_>) -> GlobalBonuses {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("mastermind".into());
    state.primary.id = Some(build.set.into());
    let mut pick = SelectedPower::picked(build.summon, build.set, 12);
    pick.slots = build
        .slots
        .iter()
        .map(|stat| Some(Enhancement::generic_io(*stat, None, 0)))
        .collect();
    state.primary.powers = vec![pick];
    for (i, other) in build.also.iter().enumerate() {
        state
            .primary
            .powers
            .push(SelectedPower::picked(*other, build.set, 20 + i as u8));
    }
    if build.switched_on {
        state.combat.power_state.insert(
            coh_math::buff_pets::buff_pet_toggle_key(build.set, build.summon),
            true,
        );
    }
    let g = coh_math::recalculate(&state, db).bonuses;
    assert!(g.errors.is_empty(), "{dataset:?}: {:?}", g.errors);
    g
}

fn protector(db: &PowerDatabase, dataset: DatasetId, slots: &[&str], on: bool) -> GlobalBonuses {
    totals(
        db,
        dataset,
        Build {
            set: ROBOTICS,
            summon: "Protector_Bots",
            slots,
            also: &[],
            switched_on: on,
        },
    )
}

fn demons(
    db: &PowerDatabase,
    dataset: DatasetId,
    slots: &[&str],
    also: &[&str],
    on: bool,
) -> GlobalBonuses {
    totals(
        db,
        dataset,
        Build {
            set: DEMONS,
            summon: "Summon_Demons",
            slots,
            also,
            switched_on: on,
        },
    )
}

#[test]
fn force_shield_reaches_the_masterminds_defense() {
    for dataset in FORKS {
        let db = load(dataset);
        let off = protector(&db, dataset, &[], false);
        let on = protector(&db, dataset, &[], true);
        assert_eq!(
            off.defense_melee, 0.0,
            "{dataset:?}: switched off still folds"
        );
        assert!(
            on.defense_melee > 0.0,
            "{dataset:?}: Force Shield folded nothing"
        );
        assert!(
            on.defense_ranged > 0.0 && on.defense_aoe > 0.0,
            "{dataset:?}"
        );
        assert!(
            on.defense_smashing > 0.0 && on.defense_negative > 0.0,
            "{dataset:?}"
        );
    }
}

/// Rebirth is out: its export carries `CopyBoosts` on no power at all (0 files against 952-1491 on
/// the other forks), so no Rebirth summon hands its slotting to its pet. That is a parser gap, not
/// a fact about Protector Bots, and it is logged on its own.
#[test]
fn force_shield_takes_the_summons_defense_slotting() {
    for dataset in [
        DatasetId::Homecoming,
        DatasetId::Thunderspy,
        DatasetId::Brainstorm,
    ] {
        let db = load(dataset);
        let base = protector(&db, dataset, &[], true);
        let slotted = protector(&db, dataset, &["Defense", "Defense", "Defense"], true);
        assert!(
            slotted.defense_melee > base.defense_melee * 1.4,
            "{dataset:?}: {} -> {} ignores the slotted Defense",
            base.defense_melee,
            slotted.defense_melee
        );
    }
}

/// Force Shield is `Collective` + `Replace`: the second bot's shield replaces the first's, so the
/// owner carries ONE. 1.3 x Ranged_Buff_Def on a henchman_lt, Homecoming L50.
#[test]
fn two_protector_bots_are_one_force_shield() {
    let db = load(DatasetId::Homecoming);
    let on = protector(&db, DatasetId::Homecoming, &[], true);
    let one_shield = 1.3 * table(&db, "henchman_lt", "Ranged_Buff_Def") * 100.0;
    assert!(
        (on.defense_melee - one_shield).abs() < 1e-6,
        "melee defense {} is not one Force Shield ({one_shield})",
        on.defense_melee
    );
}

/// The owner gets the all-allies shield AND the owner-only one: fire is (0.666 + 1.333) where
/// smashing is (0.5 + 1.0). One shield alone would read 1.332 or 1.333 here, not 1.3327.
#[test]
fn the_owner_wears_both_ember_shields() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        let off = demons(&db, dataset, &[], &[], false);
        let on = demons(&db, dataset, &[], &[], true);
        assert_eq!(
            off.resistance_fire, 0.0,
            "{dataset:?}: switched off still folds"
        );
        let per = table(&db, "henchman_lt", "Ranged_Res_Dmg") * 100.0;
        for (got, scale, what) in [
            (on.resistance_smashing, 1.5, "smashing"),
            (on.resistance_lethal, 1.5, "lethal"),
            (on.resistance_toxic, 1.5, "toxic"),
            (on.resistance_fire, 1.999, "fire"),
            (on.resistance_cold, 0.999, "cold"),
        ] {
            assert!(
                (got - scale * per).abs() < 1e-6,
                "{dataset:?} {what}: {got} is not both shields ({})",
                scale * per
            );
        }
        assert_eq!(
            on.resistance_energy, 0.0,
            "{dataset:?}: energy is not shielded"
        );
    }
    for dataset in [DatasetId::Rebirth, DatasetId::Thunderspy] {
        let db = load(dataset);
        let on = demons(&db, dataset, &[], &[], true);
        assert!(
            on.resistance_fire > 0.0,
            "{dataset:?}: Ember Shield folded nothing"
        );
    }
}

/// Before Enchant Demon the shields are IgnoreStrength, so slotting moves nothing. Enchant Demon
/// revokes them and grants boosted versions, which take the summon's Resistance slotting.
#[test]
fn enchant_demon_lets_resistance_slotting_raise_the_ember_shields() {
    for dataset in [DatasetId::Homecoming, DatasetId::Brainstorm] {
        let db = load(dataset);
        let res = ["Resistance", "Resistance", "Resistance"];
        let base = demons(&db, dataset, &[], &[], true);
        let slotted = demons(&db, dataset, &res, &[], true);
        assert_eq!(
            base.resistance_fire, slotted.resistance_fire,
            "{dataset:?}: the un-upgraded shield took slotting"
        );
        let upgraded = demons(&db, dataset, &[], &["Enchant_Demon"], true);
        assert_eq!(
            base.resistance_fire, upgraded.resistance_fire,
            "{dataset:?}: the upgrade alone changed the base numbers, so the shields doubled"
        );
        let upgraded_slotted = demons(&db, dataset, &res, &["Enchant_Demon"], true);
        assert!(
            upgraded_slotted.resistance_fire > base.resistance_fire * 1.4,
            "{dataset:?}: {} -> {} ignores the slotted Resistance after Enchant Demon",
            base.resistance_fire,
            upgraded_slotted.resistance_fire
        );
    }
}

/// The switch is offered on exactly the two summons whose henchman buffs its owner, and on no
/// other Mastermind summon.
#[test]
fn only_protector_bots_and_summon_demons_offer_the_switch() {
    let db = load(DatasetId::Homecoming);
    let mut offered = Vec::new();
    let mut state = CharacterState::empty(DatasetId::Homecoming);
    state.archetype.id = Some("mastermind".into());
    for set in db
        .powersets
        .iter()
        .filter(|s| s.id.starts_with("mastermind/"))
    {
        for power in &set.powers {
            if power.summon().is_some()
                && !coh_math::buff_pets::buff_pet_sources(power, &db, &state).is_empty()
            {
                offered.push(power.internal_name.clone().unwrap_or_default());
            }
        }
    }
    offered.sort();
    for wanted in ["Protector_Bots", "Summon_Demons"] {
        assert!(
            offered.iter().any(|p| p == wanted),
            "{wanted} not offered: {offered:?}"
        );
    }
    for henchman_summon in [
        "Battle_Drones",
        "Assault_Bot",
        "Summon_Demonlings",
        "Summon_Demon_Prince",
    ] {
        assert!(
            !offered.iter().any(|p| p == henchman_summon),
            "{henchman_summon} offered: {offered:?}"
        );
    }
}

fn table(db: &PowerDatabase, class: &str, name: &str) -> f64 {
    db.at_tables
        .value(coh_data::TableScope::Pet(class), name, 50)
        .unwrap_or_else(|| panic!("no {name} for {class}"))
}
