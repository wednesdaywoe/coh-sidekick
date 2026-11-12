//! The in-game colour of a slotted enhancement, for the phone's slot circles (phone-slot-cards).
//!
//! The game paints every enhancement on a coloured ball by what it boosts: red for damage,
//! yellow for accuracy, purple for defense. Set pieces take their set type's ball, so every
//! damage set is red. The colours are read off that art (`assets/img/Enhancements/Components/
//! POGS`), from the band across each ball's middle where the true colour sits.

use crate::shell::Db;
use coh_data::{Enhancement, EnhancementKind};

/// One ball per enhancement stat: (the art's name, its colour).
const POGS: [(&str, &str); 32] = [
    ("ACCURACY", "#e0db00"),
    ("BUFF_DAMAGE", "#d16b6b"),
    ("BUFF_DEFENSE", "#a268b0"),
    ("BUFF_TO_HIT", "#d0ca74"),
    ("CONE_RANGE", "#59772b"),
    ("CONFUSION_DURATION", "#7002ce"),
    ("DAMAGE", "#f00000"),
    ("DAMAGE_RESIST", "#00c7c8"),
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
/// archetype sets, the Universal ones, rez, rest and teleport), whose pieces go by their own
/// first stat instead.
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
        "Healing" | "Accurate Healing" => "HEAL",
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

/// A set piece's colour: its set type's, or else its first stat that has one.
fn piece_colour(set_type: Option<&str>, aspects: &[String]) -> Option<&'static str> {
    set_type
        .and_then(set_type_pog)
        .or_else(|| aspects.iter().find_map(|stat| stat_pog(stat)))
}

/// The colour a slotted enhancement shows in its circle. `None` when nothing it carries has a
/// colour (a pure proc out of a mixed-purpose set), which the circle draws as plain filled.
pub fn slot_colour(enhancement: &Enhancement, database: &Db) -> Option<&'static str> {
    match &enhancement.kind {
        EnhancementKind::IoSet {
            set_id, aspects, ..
        } => {
            let set_type = database
                .io_sets
                .as_ref()
                .and_then(|catalog| catalog.get(set_id))
                .map(|set| set.set_type.as_str());
            piece_colour(set_type, aspects)
        }
        EnhancementKind::GenericIo { stat, .. } | EnhancementKind::Origin { stat, .. } => {
            stat_pog(stat)
        }
        EnhancementKind::Special { aspects, .. } => {
            aspects.iter().find_map(|aspect| stat_pog(&aspect.stat))
        }
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

    /// Every set piece on every server gets a colour, except a pure proc out of a set with no
    /// colour of its own. A new set type, or a stat spelled a new way, fails here.
    #[test]
    fn every_set_piece_has_a_colour() {
        for (dataset, database) in every_server() {
            let catalog = database.io_sets.as_ref().expect("io sets");
            for (id, set) in &catalog.sets {
                for piece in &set.pieces {
                    let pure_proc = piece.proc && piece.aspects.is_empty();
                    if pure_proc && set_type_pog(&set.set_type).is_none() {
                        continue;
                    }
                    assert!(
                        piece_colour(Some(&set.set_type), &piece.aspects).is_some(),
                        "{} {id} ({}) piece {} {:?} has no colour",
                        dataset.as_str(),
                        set.set_type,
                        piece.num,
                        piece.aspects
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
        // A mixed-purpose set goes by the piece's own first stat.
        assert_eq!(
            piece_colour(Some("Tanker Archetype Sets"), &["Defense".into()]),
            pog("BUFF_DEFENSE")
        );
    }
}
