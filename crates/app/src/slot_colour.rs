//! The in-game colour of a slotted enhancement, for the phone's slot circles (phone-slot-cards).
//!
//! The game paints every enhancement on a coloured ball by what it boosts: red for damage,
//! yellow for accuracy, purple for defense. Set pieces take their set type's ball, so every
//! damage set is red. The colours are read off that art (`assets/img/Enhancements/Components/
//! POGS`), from the band across each ball's middle where the true colour sits, and checked
//! against the game's own set icons, which agree on every hue but resistance's (see `POGS`).

use crate::shell::Db;
use coh_data::{Enhancement, EnhancementKind, IoSet};

/// One ball per enhancement stat: (the art's name, its colour).
const POGS: [(&str, &str); 32] = [
    ("ACCURACY", "#e0db00"),
    ("BUFF_DAMAGE", "#d16b6b"),
    ("BUFF_DEFENSE", "#a268b0"),
    ("BUFF_TO_HIT", "#d0ca74"),
    ("CONE_RANGE", "#59772b"),
    ("CONFUSION_DURATION", "#7002ce"),
    ("DAMAGE", "#f00000"),
    // Not the art: `E_POG_DAMAGE_RESIST.png` is teal, which no resistance set in the game wears.
    // The game's resistance icons sit on a lavender barely pinker than Defense's; the owner chose
    // to push it to a clear pink (2026-11-12) so the two read apart on a small dot.
    ("DAMAGE_RESIST", "#d86fb8"),
    ("DEBUFF_DAMAGE", "#800000"),
    ("DEBUFF_DEFENSE", "#500d5e"),
    ("DEBUFF_TO_HIT", "#857e00"),
    ("END_DISCOUNT", "#649bf0"),
    ("END_DRAIN", "#002560"),
    ("FEAR_DURATION", "#5d003c"),
    ("FLY_SPEED", "#35997c"),
    ("HEAL", "#00d303"),
    ("HOLD_DURATION", "#f08700"),
    ("IMMOBILIZATION_DURATION", "#916200"),
    ("INTAGIBILITY_DURATION", "#ebb36a"),
    ("INTERRUPT_TIMES", "#6c5952"),
    ("JUMP_DISTANCE", "#d1af53"),
    ("KNOCKBACK_DISTANCE", "#747f8a"),
    ("LEAP_HEIGHT", "#a1853a"),
    ("RADIUS", "#e40b81"),
    ("RANGE_INCREASE", "#859e5f"),
    ("RECHARGE_TIME", "#43261f"),
    ("RECOVERY", "#0c20a2"),
    ("RUN_SPEED", "#78b9a5"),
    ("SLEEP_DURATION", "#8b8b8b"),
    ("SLOW_MOVEMENT", "#1f5c4a"),
    ("STUN_DURATION", "#565656"),
    ("TAUNT_DURATION", "#000000"),
];

fn pog(name: &str) -> Option<&'static str> {
    POGS.iter().find(|(n, _)| *n == name).map(|(_, hex)| *hex)
}

/// The ball for one enhancement stat, in any of the spellings the data uses for it (a set
/// piece's aspects, a common IO's type, a special's aspect).
fn stat_pog(stat: &str) -> Option<&'static str> {
    let name = match stat {
        "Accuracy" => "ACCURACY",
        "Damage" => "DAMAGE",
        "Defense" => "BUFF_DEFENSE",
        "Defense Debuff" => "DEBUFF_DEFENSE",
        "ToHit" => "BUFF_TO_HIT",
        "ToHit Debuff" => "DEBUFF_TO_HIT",
        "Damage Resistance" | "Resistance" => "DAMAGE_RESIST",
        "Heal" | "Healing" | "Absorb" => "HEAL",
        "Recharge" => "RECHARGE_TIME",
        "Endurance" | "EnduranceReduction" => "END_DISCOUNT",
        "EndMod" | "EnduranceModification" => "RECOVERY",
        "Hold" | "Mez" => "HOLD_DURATION",
        "Stun" => "STUN_DURATION",
        "Sleep" => "SLEEP_DURATION",
        "Immobilize" => "IMMOBILIZATION_DURATION",
        "Confuse" => "CONFUSION_DURATION",
        "Fear" | "Terrorize" => "FEAR_DURATION",
        "Taunt" | "Threat" => "TAUNT_DURATION",
        "Knockback" | "KnockToKnockDown" => "KNOCKBACK_DISTANCE",
        "Slow" => "SLOW_MOVEMENT",
        "Run" | "Run Speed" | "Move Speed" => "RUN_SPEED",
        "Jump" => "JUMP_DISTANCE",
        "Fly" | "+Fly Magnitude" => "FLY_SPEED",
        "Range" => "RANGE_INCREASE",
        "Interrupt" | "InterruptTime" => "INTERRUPT_TIMES",
        "Intangible" => "INTAGIBILITY_DURATION",
        _ => return None,
    };
    pog(name)
}

/// The ball a set type wears in the game. `None` for the types that mix purposes (the
/// archetype sets, the Universal ones, rez and teleport), which take the stat most of their
/// pieces boost instead ([`set_colour`]).
fn set_type_pog(set_type: &str) -> Option<&'static str> {
    let name = match set_type {
        "Melee Damage"
        | "Melee AoE Damage"
        | "PBAoE Damage"
        | "Ranged Damage"
        | "Ranged AoE Damage"
        | "Targeted AoE Damage"
        | "Sniper Attacks"
        | "Pet Damage"
        | "Recharge Intensive Pets"
        | "Universal Damage Sets" => "DAMAGE",
        "Defense Sets" => "BUFF_DEFENSE",
        "Defense Debuff" | "Accurate Defense Debuff" => "DEBUFF_DEFENSE",
        "To Hit Buff" => "BUFF_TO_HIT",
        "To Hit Debuff" | "Accurate To-Hit Debuff" => "DEBUFF_TO_HIT",
        "Resist Damage" => "DAMAGE_RESIST",
        // Rebirth's Rest Buff set is one proc that restores health out of combat; Rest heals.
        "Healing" | "Accurate Healing" | "Rest Buff" => "HEAL",
        "Endurance Modification" => "RECOVERY",
        "Holds" => "HOLD_DURATION",
        "Stuns" => "STUN_DURATION",
        "Sleep" => "SLEEP_DURATION",
        "Immobilize" => "IMMOBILIZATION_DURATION",
        "Confuse" => "CONFUSION_DURATION",
        "Fear" => "FEAR_DURATION",
        "Taunt" | "Threat Duration" => "TAUNT_DURATION",
        "Knockback" => "KNOCKBACK_DISTANCE",
        "Slow Movement" => "SLOW_MOVEMENT",
        "Running" | "Running & Sprints" => "RUN_SPEED",
        "Leaping" | "Leaping & Sprints" => "JUMP_DISTANCE",
        "Flight" => "FLY_SPEED",
        _ => return None,
    };
    pog(name)
}

/// A set's one colour, worn by every piece: its type's, or for a type with none (the archetype
/// and Universal sets) the stat most of its pieces boost, the earlier stat winning a tie. One
/// colour per set, because per-piece colours made an archetype set read as alternating stripes.
fn set_colour(set: &IoSet) -> Option<&'static str> {
    if let Some(colour) = set_type_pog(&set.set_type) {
        return Some(colour);
    }
    let mut tally: Vec<(&'static str, usize)> = Vec::new();
    for colour in set
        .pieces
        .iter()
        .flat_map(|piece| piece.aspects.iter().filter_map(|stat| stat_pog(stat)))
    {
        match tally.iter_mut().find(|(seen, _)| *seen == colour) {
            Some((_, count)) => *count += 1,
            None => tally.push((colour, 1)),
        }
    }
    // `max_by_key` keeps the LAST of equals, so walk it reversed to let the first seen win.
    tally
        .into_iter()
        .rev()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| colour)
}

/// How a slotted enhancement's circle is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SlotColour {
    /// The in-game colour of what it boosts. `None` when nothing it carries has one.
    pub fill: Option<&'static str>,
    /// A piece of a set the game frames in purple (very rare, Superior archetype or event), which
    /// the circle rings in purple: the fill already uses purple for Defense, the ring does not.
    pub superior: bool,
}

pub fn slot_colour(enhancement: &Enhancement, database: &Db) -> SlotColour {
    match &enhancement.kind {
        EnhancementKind::IoSet {
            set_id, aspects, ..
        } => match database
            .io_sets
            .as_ref()
            .and_then(|catalog| catalog.get(set_id))
        {
            Some(set) => SlotColour {
                fill: set_colour(set),
                superior: crate::view::icons::io_set_is_superior(&set.category, &set.icon),
            },
            // A set this dataset doesn't carry: the piece's own first stat is all there is.
            None => SlotColour {
                fill: aspects.iter().find_map(|stat| stat_pog(stat)),
                superior: false,
            },
        },
        EnhancementKind::GenericIo { stat, .. } | EnhancementKind::Origin { stat, .. } => {
            SlotColour {
                fill: stat_pog(stat),
                superior: false,
            }
        }
        EnhancementKind::Special { aspects, .. } => SlotColour {
            fill: aspects.iter().find_map(|aspect| stat_pog(&aspect.stat)),
            superior: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{DatasetId, PowerDatabase};
    use std::path::PathBuf;

    fn every_server() -> Vec<(DatasetId, PowerDatabase)> {
        DatasetId::ALL
            .into_iter()
            .map(|dataset| {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "../../contract/{}/bundle.json.gz",
                    dataset.as_str()
                ));
                let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
                (
                    dataset,
                    PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads"),
                )
            })
            .collect()
    }

    /// Every set on every server gets one colour, so no piece draws plain. A new set type, or a
    /// stat spelled a new way, fails here.
    #[test]
    fn every_set_has_a_colour() {
        for (dataset, database) in every_server() {
            let catalog = database.io_sets.as_ref().expect("io sets");
            for (id, set) in &catalog.sets {
                assert!(
                    set_colour(set).is_some(),
                    "{} {id} ({}) has no colour",
                    dataset.as_str(),
                    set.set_type
                );
            }
        }
    }

    /// An archetype set is one colour, not one per piece: Might of the Tanker's pieces lead with
    /// Accuracy or Damage in turn, and it is a damage set, so all red.
    #[test]
    fn an_archetype_set_is_one_colour() {
        let (_, database) = every_server().remove(0);
        let catalog = database.io_sets.as_ref().expect("io sets");
        let set = catalog
            .sets
            .values()
            .find(|set| set.name == "Superior Might of the Tanker")
            .expect("Homecoming carries Superior Might of the Tanker");
        assert_eq!(set_colour(set), pog("DAMAGE"));
    }

    /// The purple ring goes on exactly the sets the desktop draws in a superior frame.
    #[test]
    fn the_ring_matches_the_superior_frame() {
        for (dataset, database) in every_server() {
            let catalog = database.io_sets.as_ref().expect("io sets");
            for (id, set) in &catalog.sets {
                for attuned in [false, true] {
                    let frame =
                        crate::view::icons::io_set_overlay_url(&set.category, &set.icon, attuned);
                    assert_eq!(
                        crate::view::icons::io_set_is_superior(&set.category, &set.icon),
                        frame.contains("superior"),
                        "{} {id}: ring and frame disagree ({frame})",
                        dataset.as_str()
                    );
                }
            }
        }
    }

    /// Every common IO type and every special's first aspect has a colour, so a slot holding
    /// any of them never draws plain.
    #[test]
    fn every_single_stat_and_special_has_a_colour() {
        for (dataset, database) in every_server() {
            let catalog = database.enhancements.as_ref().expect("enhancements");
            for stat in &catalog.common_io_types {
                assert!(
                    stat_pog(stat).is_some(),
                    "{} common IO {stat} has no colour",
                    dataset.as_str()
                );
            }
            for (family, _, defs) in catalog.special_families() {
                for (id, def) in defs {
                    assert!(
                        def.aspects.iter().any(|a| stat_pog(&a.stat).is_some()),
                        "{} {family} {id} has no coloured aspect",
                        dataset.as_str()
                    );
                }
            }
        }
    }

    /// The colours the owner named: damage sets red, defense purple, accuracy yellow.
    #[test]
    fn damage_sets_are_red_and_defense_purple() {
        assert_eq!(set_type_pog("Melee Damage"), pog("DAMAGE"));
        assert_eq!(set_type_pog("Targeted AoE Damage"), pog("DAMAGE"));
        assert_eq!(set_type_pog("Defense Sets"), pog("BUFF_DEFENSE"));
        assert_eq!(stat_pog("Accuracy"), Some("#e0db00"));
    }
}
