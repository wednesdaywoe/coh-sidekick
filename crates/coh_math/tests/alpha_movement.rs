//! An Alpha's run / fly / jump boost enhances the travel power that carries the speed, the way
//! a slotted IO of that kind does, and nothing else. The boosts used to be filed under names the
//! movement step never read, so no Alpha moved a speed.

use coh_data::{
    CharacterState, DatasetId, Enhancement, IncarnateSlot, PoolSelection, PowerDatabase,
    SelectedPower,
};
use std::path::PathBuf;

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

/// Run speed, fly speed, jump speed, jump height for a level-50 Blaster with one travel toggle
/// running (or none), an optional active Alpha, and an optional single level-50 IO in it.
fn movement(
    db: &PowerDatabase,
    dataset: DatasetId,
    travel: Option<(&str, &str)>,
    alpha: Option<&str>,
    io: Option<&str>,
) -> [f64; 4] {
    let mut state = CharacterState::empty(dataset);
    state.level = 50;
    state.archetype.id = Some("blaster".into());
    if let Some((pool, power)) = travel {
        let mut pick = SelectedPower::picked(power, pool, 4);
        pick.is_active = true;
        if let Some(aspect) = io {
            pick.slots = vec![Some(Enhancement::generic_io(aspect, None, 0))];
        }
        state.pools = vec![PoolSelection {
            id: pool.into(),
            name: pool.into(),
            powers: vec![pick],
        }];
    }
    state.incarnates.alpha = alpha.map(|name| IncarnateSlot {
        power_name: name.into(),
        active: true,
    });
    let b = coh_math::recalculate(&state, db).bonuses;
    [b.run_speed, b.fly_speed, b.jump_speed, b.jump_height]
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= want.abs() * 1e-6 + 1e-9
}

/// A Radial Alpha's movement boost is +33% enhancement. With nothing else slotted, the travel
/// power's whole buff scales by 1.33 (every case here is a power with no unenhanceable part:
/// Rebirth's Fly has one, so it is left out); axes the Alpha does not name do not move.
#[test]
fn radial_alpha_enhances_the_travel_power_it_names() {
    // (dataset, pool, power, axes that power carries, Alpha, axes that Alpha boosts)
    let cases: [(DatasetId, &str, &str, &[usize], &str, &[usize]); 6] = [
        (
            DatasetId::Homecoming,
            "speed",
            "Super_Speed",
            &[0],
            "Musculature_Radial_Paragon",
            &[0],
        ),
        (
            DatasetId::Homecoming,
            "flight",
            "Fly",
            &[1],
            "Nerve_Radial_Paragon",
            &[1],
        ),
        (
            DatasetId::Homecoming,
            "flight",
            "Fly",
            &[1],
            "Musculature_Radial_Paragon",
            &[0],
        ),
        (
            DatasetId::Rebirth,
            "leaping",
            "Leap",
            &[2, 3],
            "Spiritual_Radial_Paragon",
            &[2, 3],
        ),
        (
            DatasetId::Rebirth,
            "speed",
            "Super_Speed",
            &[0],
            "Agility_Radial_Paragon",
            &[0, 1, 2, 3],
        ),
        (
            DatasetId::Homecoming,
            "flight",
            "Fly",
            &[1],
            "Agility_Radial_Paragon",
            &[0, 1, 2, 3],
        ),
    ];
    for (dataset, pool, power, carries, alpha, boosts) in cases {
        let db = load(dataset);
        let bare = movement(&db, dataset, Some((pool, power)), None, None);
        let with = movement(&db, dataset, Some((pool, power)), Some(alpha), None);
        for axis in carries.iter().copied() {
            assert!(
                bare[axis] > 0.0,
                "{dataset:?} {power} axis {axis} carries no speed"
            );
            let want = if boosts.contains(&axis) {
                bare[axis] * 1.33
            } else {
                bare[axis]
            };
            assert!(
                close(with[axis], want),
                "{dataset:?} {power} + {alpha}, axis {axis}: {} (bare {}), expected {want}",
                with[axis],
                bare[axis]
            );
        }
    }
}

/// The Alpha adds to what is slotted (its +33% lands beside the IO's, below the diminishing-
/// returns knee here), rather than multiplying the finished total.
#[test]
fn radial_alpha_adds_to_a_slotted_io() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let db = load(dataset);
        let ss = Some(("speed", "Super_Speed"));
        let bare = movement(&db, dataset, ss, None, None)[0];
        let io = movement(&db, dataset, ss, None, Some("Run Speed"))[0];
        let both = movement(
            &db,
            dataset,
            ss,
            Some("Musculature_Radial_Paragon"),
            Some("Run Speed"),
        )[0];
        let want = bare * (io / bare + 0.33);
        assert!(
            close(both, want),
            "{dataset:?}: IO + Alpha {both}, expected {want} (bare {bare}, IO {io})"
        );
    }
}

/// An Alpha is an enhancement, not a buff: with no travel power running there is nothing for
/// it to enhance, and an Alpha with no movement boost changes no speed at all.
#[test]
fn alpha_alone_grants_no_speed() {
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let db = load(dataset);
        let none = movement(&db, dataset, None, None, None);
        let musc = movement(&db, dataset, None, Some("Agility_Radial_Paragon"), None);
        assert_eq!(
            musc, none,
            "{dataset:?}: Agility Radial with no travel power"
        );

        let ss = Some(("speed", "Super_Speed"));
        let bare = movement(&db, dataset, ss, None, None);
        let core = movement(&db, dataset, ss, Some("Musculature_Core_Paragon"), None);
        assert_eq!(
            core, bare,
            "{dataset:?}: Musculature Core has no movement boost"
        );
    }
}
