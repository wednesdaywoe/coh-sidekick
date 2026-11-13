//! Writing a build out as a Mids `.mbd`.
//!
//! Mids answers an ID it doesn't know with an empty slot and no error, so every ID written here
//! comes from Mids' own table. The table is read backwards through the importer's own lookup,
//! which means the writer can only hand Mids an ID that reads back as the piece it started from.

use crate::boost_index::{BoostEntry, BoostIndex};
use crate::enhancements::EnhancementCatalog;
use crate::level::Level;
use crate::mbd::MbdEnhancement;
use crate::mbd_import::{catalog_stat, locate, mids_grade, relative_level_for};
use crate::mids_uids::MidsUids;
use crate::skif::SkifEnhancement;
use std::collections::HashMap;

/// What one of our slotted pieces is, as far as Mids can tell pieces apart.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PieceKey {
    Set {
        set: String,
        piece: u8,
        attuned: bool,
    },
    Generic {
        stat: String,
    },
    Origin {
        stat: String,
    },
    Special {
        category: String,
        id: String,
    },
}

/// Our pieces -> the UID Mids carries for each, for one fork.
#[derive(Debug, Clone)]
pub struct MidsEnhancementIds {
    by_piece: HashMap<PieceKey, String>,
    io_set_levels: HashMap<String, [u8; 2]>,
    generic_io_levels: Option<[u8; 2]>,
}

/// Something Mids will hold differently from how the build has it. The file is still written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PieceLoss {
    /// Mids carries this set piece in one form only, so an attuned piece goes out crafted or
    /// the other way round.
    Attunement { written_attuned: bool },
    /// The level is outside the range Mids allows for it, and Mids will clamp it on load
    /// (MBDEXPORT-22). The file still states ours.
    LevelClamped { asked: u8, kept: u8 },
    /// Mids' relative levels run -3 to +5.
    RelativeLevelClamped { asked: i8, kept: i8 },
}

/// One piece ready for a slot, with anything Mids will hold differently.
#[derive(Debug, Clone, PartialEq)]
pub struct MidsPiece {
    pub enhancement: MbdEnhancement,
    pub losses: Vec<PieceLoss>,
}

/// Why a piece can't be written at all. The slot goes out empty.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteRefusal {
    #[error("Mids' database has no enhancement that reads back as this one")]
    NoMidsId,
    #[error("{0:?} is not an origin tier")]
    Tier(String),
}

/// Mids writes these for a piece with no crafted level, measured on Mids 3.8.6's own files.
/// It doesn't read them back (MBDEXPORT-17), so they're only here to look like Mids' output.
const MIDS_UNLEVELLED_IO_LEVEL: i32 = 1;
const MIDS_SPECIAL_GRADE: &str = "SingleO";
const MIDS_NO_GRADE: &str = "None";

impl MidsEnhancementIds {
    /// Build the reverse table by running every UID Mids carries through the importer's lookup.
    ///
    /// Errors where two UIDs read back as the same piece, since then nothing says which to write.
    pub fn new(
        mids: &MidsUids,
        index: &BoostIndex,
        catalog: &EnhancementCatalog,
    ) -> Result<Self, String> {
        let mut by_piece: HashMap<PieceKey, String> = HashMap::new();
        for uid in mids.uids() {
            // A UID the importer can't place (MBDIMPORT-9's twelve) is one the writer can't use.
            let Ok(entry) = locate(uid, index, mids) else {
                continue;
            };
            let Some(key) = key_of(entry, catalog) else {
                continue;
            };
            if let Some(prior) = by_piece.insert(key.clone(), uid.clone()) {
                return Err(format!(
                    "Mids' {prior} and {uid} both read back as {key:?}; nothing says which to write"
                ));
            }
        }
        let io_set_levels = by_piece
            .keys()
            .filter_map(|key| match key {
                PieceKey::Set { set, .. } => Some(set),
                _ => None,
            })
            .filter_map(|set| Some((set.clone(), mids.io_set_levels(set)?)))
            .collect();
        Ok(Self {
            by_piece,
            io_set_levels,
            generic_io_levels: mids.generic_io_levels(),
        })
    }

    /// How many of our pieces have a Mids UID.
    pub fn len(&self) -> usize {
        self.by_piece.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_piece.is_empty()
    }

    /// The Mids slot contents for one of our pieces.
    ///
    /// `default_io_level` is the level a crafted piece that states none is written at — the
    /// build's global IO level.
    pub fn mids_uid_for(
        &self,
        enhancement: &SkifEnhancement,
        default_io_level: Level,
    ) -> Result<MidsPiece, WriteRefusal> {
        let mut losses = Vec::new();
        let written = match enhancement {
            SkifEnhancement::IoSet {
                set_id,
                piece_num,
                attuned,
                level,
                booster,
            } => {
                let (uid, written_attuned) = self.set_piece(set_id, *piece_num, *attuned)?;
                if written_attuned != *attuned {
                    losses.push(PieceLoss::Attunement { written_attuned });
                }
                let range = self.io_set_levels.get(set_id).copied();
                let top = || i32::from(range.map_or(default_io_level.get(), |[_, high]| high)) - 1;
                let io_level = match (*attuned, written_attuned) {
                    // Mids writes the top of the set's range on an attuned piece, and ignores it.
                    (_, true) => top(),
                    // An attuned piece Mids only has crafted: the top is nearest to what it was.
                    (true, false) => top(),
                    (false, false) => {
                        crafted_level(level.unwrap_or(default_io_level), range, &mut losses)
                    }
                };
                mids_enhancement(
                    uid,
                    MIDS_NO_GRADE,
                    io_level,
                    booster_token(*booster, &mut losses),
                )
            }

            SkifEnhancement::IoGeneric {
                stat,
                level,
                booster,
            } => {
                let uid = self.uid(PieceKey::Generic { stat: stat.clone() })?;
                let io_level = crafted_level(
                    level.unwrap_or(default_io_level),
                    self.generic_io_levels,
                    &mut losses,
                );
                mids_enhancement(
                    uid,
                    MIDS_NO_GRADE,
                    io_level,
                    booster_token(*booster, &mut losses),
                )
            }

            SkifEnhancement::Origin {
                stat,
                tier,
                relative_level,
            } => {
                let uid = self.uid(PieceKey::Origin { stat: stat.clone() })?;
                let grade = mids_grade(tier).ok_or_else(|| WriteRefusal::Tier(tier.clone()))?;
                mids_enhancement(
                    uid,
                    grade,
                    MIDS_UNLEVELLED_IO_LEVEL,
                    offset_token(*relative_level, &mut losses),
                )
            }

            SkifEnhancement::Special {
                category,
                id,
                relative_level,
            } => {
                let uid = self.uid(PieceKey::Special {
                    category: category.clone(),
                    id: id.clone(),
                })?;
                mids_enhancement(
                    uid,
                    MIDS_SPECIAL_GRADE,
                    MIDS_UNLEVELLED_IO_LEVEL,
                    offset_token(*relative_level, &mut losses),
                )
            }
        };
        Ok(MidsPiece {
            enhancement: written,
            losses,
        })
    }

    fn uid(&self, key: PieceKey) -> Result<&str, WriteRefusal> {
        self.by_piece
            .get(&key)
            .map(String::as_str)
            .ok_or(WriteRefusal::NoMidsId)
    }

    /// The piece in the form asked for, or else the one form Mids has, and which it was.
    fn set_piece(&self, set: &str, piece: u8, attuned: bool) -> Result<(&str, bool), WriteRefusal> {
        let form = |attuned| {
            self.uid(PieceKey::Set {
                set: set.to_string(),
                piece,
                attuned,
            })
            .map(|uid| (uid, attuned))
        };
        form(attuned).or_else(|_| form(!attuned))
    }
}

fn key_of(entry: &BoostEntry, catalog: &EnhancementCatalog) -> Option<PieceKey> {
    match entry {
        BoostEntry::IoSet {
            set,
            piece,
            attuned,
        } => Some(PieceKey::Set {
            set: set.clone(),
            piece: *piece,
            attuned: *attuned,
        }),
        // Only the level-less template; Mids states the level in the slot.
        BoostEntry::CommonIo { level: None, stats } => Some(PieceKey::Generic {
            stat: catalog_stat(stats, catalog).ok()?,
        }),
        BoostEntry::CommonIo { level: Some(_), .. } => None,
        BoostEntry::Origin { stats, .. } => Some(PieceKey::Origin {
            stat: catalog_stat(stats, catalog).ok()?,
        }),
        BoostEntry::Special { family, id } => Some(PieceKey::Special {
            category: family.clone(),
            id: id.clone(),
        }),
        BoostEntry::Unclassified => None,
    }
}

fn mids_enhancement(uid: &str, grade: &str, io_level: i32, relative_level: &str) -> MbdEnhancement {
    MbdEnhancement {
        uid: uid.to_string(),
        enhancement: String::new(),
        grade: grade.to_string(),
        io_level,
        relative_level: relative_level.to_string(),
        obtained: false,
    }
}

/// Mids' 0-based `IoLevel`, noting where Mids' range for the piece will move it.
fn crafted_level(level: Level, range: Option<[u8; 2]>, losses: &mut Vec<PieceLoss>) -> i32 {
    let asked = level.get();
    if let Some([low, high]) = range {
        let kept = asked.clamp(low, high);
        if kept != asked {
            losses.push(PieceLoss::LevelClamped { asked, kept });
        }
    }
    i32::from(asked) - 1
}

/// On an invention piece Mids' relative level is the booster count.
fn booster_token(booster: u8, losses: &mut Vec<PieceLoss>) -> &'static str {
    offset_token(i8::try_from(booster).unwrap_or(i8::MAX), losses)
}

fn offset_token(asked: i8, losses: &mut Vec<PieceLoss>) -> &'static str {
    if let Some(token) = relative_level_for(asked) {
        return token;
    }
    let kept = asked.clamp(RELATIVE_LEVEL_FLOOR, RELATIVE_LEVEL_CEILING);
    losses.push(PieceLoss::RelativeLevelClamped { asked, kept });
    relative_level_for(kept).expect("the clamped offset is in Mids' range")
}

const RELATIVE_LEVEL_FLOOR: i8 = -3;
const RELATIVE_LEVEL_CEILING: i8 = 5;
