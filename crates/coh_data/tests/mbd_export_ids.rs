//! The writer's enhancement IDs: each one reads back as the piece it was written from.

use coh_data::boost_index::{BoostEntry, OriginTier};
use coh_data::level::Level;
use coh_data::mbd::{self, MbdEnhancement};
use coh_data::mbd_export::{MidsEnhancementIds, PieceLoss};
use coh_data::mbd_import::{resolve_enhancement, with_uid};
use coh_data::skif::SkifEnhancement;
use coh_data::{DatasetId, PowerDatabase};
use std::path::PathBuf;

const DEFAULT_IO_LEVEL: Level = Level::constant(50);

fn load(dataset: DatasetId) -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(dataset.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).unwrap_or_else(|e| panic!("load {dataset:?}: {e}"))
}

fn ids(database: &PowerDatabase) -> MidsEnhancementIds {
    MidsEnhancementIds::new(
        database.mids_uids.as_ref().expect("a Mids UID table"),
        database.boost_index.as_ref().expect("a boost index"),
        database
            .enhancements
            .as_ref()
            .expect("an enhancement catalog"),
    )
    .expect("no two Mids UIDs read back as one piece")
}

fn read_back(database: &PowerDatabase, written: &MbdEnhancement) -> Option<SkifEnhancement> {
    resolve_enhancement(
        written,
        database.boost_index.as_ref()?,
        database.mids_uids.as_ref()?,
        database.enhancements.as_ref()?,
    )
    .ok()
}

fn fixtures(dataset: DatasetId) -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mids")
        .join(dataset.as_str());
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "mbd"))
        .collect();
    paths.sort();
    paths
}

/// MBDEXPORT-1 and -4: every piece in a build Mids wrote goes back out under the UID, grade and
/// relative level Mids gave it. `IoLevel` is compared only on crafted pieces, the only ones
/// Mids reads it on (MBDEXPORT-17).
#[test]
fn every_piece_mids_wrote_goes_back_out_as_mids_wrote_it() {
    let mut compared = 0;
    for dataset in [DatasetId::Homecoming, DatasetId::Rebirth] {
        let database = load(dataset);
        let ids = ids(&database);
        let names = database.mids_enh_names.as_ref();
        for path in fixtures(dataset) {
            let text = std::fs::read_to_string(&path).expect("a fixture");
            let file = mbd::from_str(&text).expect("a .mbd");
            let slots = file.power_entries.iter().flat_map(|e| &e.slot_entries);
            for original in slots.filter_map(|slot| slot.enhancement.as_ref()) {
                let Ok(original) = with_uid(original, names) else {
                    continue;
                };
                // The importer's own refusals (MBDIMPORT-9) aren't the writer's to answer.
                let Some(piece) = read_back(&database, &original) else {
                    continue;
                };
                let written = ids
                    .mids_uid_for(&piece, DEFAULT_IO_LEVEL)
                    .unwrap_or_else(|e| panic!("{path:?} {}: {e}", original.uid));
                let what = format!("{path:?} {} as {piece:?}", original.uid);
                assert_eq!(written.losses, vec![], "{what}");
                let written = written.enhancement;
                assert_eq!(written.uid, original.uid, "{what}");
                // Mids wrote `None` once in the corpus; the importer reads it as even.
                let original_relative = match original.relative_level.as_str() {
                    "None" => "Even",
                    other => other,
                };
                assert_eq!(written.relative_level, original_relative, "{what}");
                let crafted = matches!(
                    piece,
                    SkifEnhancement::IoSet { attuned: false, .. }
                        | SkifEnhancement::IoGeneric { .. }
                );
                if crafted {
                    assert_eq!(written.io_level, original.io_level, "{what}");
                }
                if matches!(piece, SkifEnhancement::Origin { .. }) {
                    assert_eq!(written.grade, original.grade, "{what}");
                }
                compared += 1;
            }
        }
    }
    assert!(compared > 500, "only {compared} pieces compared");
}

/// The grade a game record would carry in a Mids file, so the importer can read it at all.
fn grade_for(entry: &BoostEntry) -> &'static str {
    match entry {
        BoostEntry::Origin { tier, .. } => match tier {
            OriginTier::Training => "TrainingO",
            OriginTier::Dual => "DualO",
            OriginTier::Single => "SingleO",
        },
        _ => "None",
    }
}

/// Every piece each fork's game data names, written and read back: it comes back as itself, or
/// as itself with a loss the writer reported, or the writer refuses it. Never as another piece.
#[test]
fn every_piece_reads_back_as_itself_or_says_why_not() {
    for dataset in DatasetId::ALL {
        let database = load(dataset);
        let ids = ids(&database);
        let index = database.boost_index.as_ref().expect("a boost index");
        let (mut exact, mut with_loss, mut refused) = (0, 0, Vec::new());
        for (name, entry) in &index.entries {
            let io_level = match entry {
                BoostEntry::CommonIo {
                    level: Some(level), ..
                } => (*level - 1) as i32,
                _ => 49,
            };
            let as_read = MbdEnhancement {
                uid: name.clone(),
                enhancement: String::new(),
                grade: grade_for(entry).to_string(),
                io_level,
                relative_level: "Even".to_string(),
                obtained: false,
            };
            let Some(piece) = read_back(&database, &as_read) else {
                continue;
            };
            let Ok(written) = ids.mids_uid_for(&piece, DEFAULT_IO_LEVEL) else {
                refused.push(name.as_str());
                continue;
            };
            let back = read_back(&database, &written.enhancement).unwrap_or_else(|| {
                panic!("{dataset:?} {name}: wrote {written:?}, which doesn't read")
            });
            // Every record is read in at level 50, which some sets can't be; that warning is right.
            let losses: Vec<&PieceLoss> = written
                .losses
                .iter()
                .filter(|loss| !matches!(loss, PieceLoss::LevelClamped { .. }))
                .collect();
            let expected = match (&piece, losses.as_slice()) {
                (
                    SkifEnhancement::IoSet {
                        set_id,
                        piece_num,
                        booster,
                        ..
                    },
                    [PieceLoss::Attunement { written_attuned }],
                ) => SkifEnhancement::IoSet {
                    set_id: set_id.clone(),
                    piece_num: *piece_num,
                    attuned: *written_attuned,
                    level: if *written_attuned {
                        None
                    } else {
                        Some(DEFAULT_IO_LEVEL)
                    },
                    booster: *booster,
                },
                (
                    SkifEnhancement::Special { relative_level, .. },
                    [PieceLoss::SwappedFor { category, id }],
                ) => SkifEnhancement::Special {
                    category: category.clone(),
                    id: id.clone(),
                    relative_level: *relative_level,
                },
                (_, []) => piece.clone(),
                (_, losses) => panic!("{dataset:?} {name}: unexpected losses {losses:?}"),
            };
            assert_eq!(normalise(back), normalise(expected), "{dataset:?} {name}");
            match losses.is_empty() {
                true => exact += 1,
                false => with_loss += 1,
            }
        }
        println!(
            "{dataset:?}: {} Mids IDs; {exact} pieces exact, {with_loss} with a reported loss, {} refused: {}",
            ids.len(),
            refused.len(),
            refused.join(" ")
        );
        assert!(
            exact > 1000,
            "{dataset:?}: only {exact} pieces round-trip exactly"
        );
    }
}

/// A crafted piece read from a record with no level gets the writer's default; compare without it.
fn normalise(piece: SkifEnhancement) -> SkifEnhancement {
    match piece {
        SkifEnhancement::IoSet {
            set_id,
            piece_num,
            attuned,
            booster,
            ..
        } => SkifEnhancement::IoSet {
            set_id,
            piece_num,
            attuned,
            level: None,
            booster,
        },
        SkifEnhancement::IoGeneric { stat, booster, .. } => SkifEnhancement::IoGeneric {
            stat,
            level: None,
            booster,
        },
        other => other,
    }
}

/// The reverse table answers on every fork, and the function the plan names exists.
#[test]
fn mids_uid_for_answers_on_every_fork() {
    for dataset in DatasetId::ALL {
        let database = load(dataset);
        assert!(ids(&database).len() > 1000, "{dataset:?}");
    }
}

/// A Synthetic Hamidon goes out as the regular Hamidon with the same effects, and says so.
#[test]
fn a_synthetic_hamidon_goes_out_as_its_regular_twin() {
    for dataset in DatasetId::ALL {
        let database = load(dataset);
        let catalog = database.enhancements.as_ref().expect("a catalog");
        let ids = ids(&database);
        assert!(!catalog.synthetic_hamidon.is_empty(), "{dataset:?}");
        for id in catalog.synthetic_hamidon.keys() {
            let piece = SkifEnhancement::Special {
                category: "synthetic-hamidon".to_string(),
                id: id.clone(),
                relative_level: 0,
            };
            let written = ids
                .mids_uid_for(&piece, DEFAULT_IO_LEVEL)
                .unwrap_or_else(|e| panic!("{dataset:?} {id}: {e}"));
            let [PieceLoss::SwappedFor { category, id: twin }] = written.losses.as_slice() else {
                panic!("{dataset:?} {id}: {:?}", written.losses);
            };
            assert_eq!(category, "hamidon", "{dataset:?} {id}");
            assert_eq!(
                catalog.hamidon[twin].aspects, catalog.synthetic_hamidon[id].aspects,
                "{dataset:?} {id}"
            );
        }
    }
}
