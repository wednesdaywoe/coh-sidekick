//! Writing a build out as a Mids `.mbd`.
//!
//! Mids answers an ID it doesn't know with an empty slot and no error, so every ID written here
//! comes from Mids' own table. The table is read backwards through the importer's own lookup,
//! which means the writer can only hand Mids an ID that reads back as the piece it started from.

use crate::boost_index::{BoostEntry, BoostIndex};
use crate::character::{AspectValue, CharacterState, SelectedPower, SlotLevelSource};
use crate::enhancements::{EnhancementCatalog, SpecialEnhancementDef};
use crate::granted_powers::requires;
use crate::level::Level;
use crate::leveling_schedule::LevelingSchedule;
use crate::mbd::{
    dataset_for_database, MbdBuiltWith, MbdEnhancement, MbdFile, MbdPowerEntry, MbdSlotEntry,
};
use crate::mbd_import::{
    catalog_stat, locate, mids_grade, mids_max_used_level, relative_level_for,
};
use crate::mids_names::{fold_path, MidsNames};
use crate::mids_uids::MidsUids;
use crate::skif::{slim_enhancement, SkifEnhancement};
use crate::slot_levels::{levels_of, slot_levels, SlotCategory, SlotLevels};
use crate::{DatasetId, InherentGrants, PowerDatabase};
use serde_json::Value;
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
    /// A special Mids has no record of -> the one Mids has that does the same thing.
    stand_ins: HashMap<PieceKey, PieceKey>,
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
    /// Mids has no record of this special, so it went out as this one, which does the same.
    SwappedFor { category: String, id: String },
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
        let stand_ins = stand_ins(&by_piece, index, catalog);
        Ok(Self {
            by_piece,
            stand_ins,
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
                let key = PieceKey::Special {
                    category: category.clone(),
                    id: id.clone(),
                };
                let uid = match self.uid(key.clone()) {
                    Ok(uid) => uid,
                    Err(refusal) => {
                        let stand_in = self.stand_ins.get(&key).ok_or(refusal)?;
                        if let PieceKey::Special { category, id } = stand_in {
                            losses.push(PieceLoss::SwappedFor {
                                category: category.clone(),
                                id: id.clone(),
                            });
                        }
                        self.uid(stand_in.clone())?
                    }
                };
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

/// Specials Mids has no record of, each paired with the one Mids has that does the same thing.
///
/// Same effects alone isn't enough: a Synthetic Nucleolus matches a Hamidon, a Titan, a Hydra
/// and a D-Sync piece. The game's record name settles it, since `Synthetic_Hamidon_Damage_Accuracy`
/// ends with Mids' `Hamidon_Damage_Accuracy`. Both must hold, for exactly one candidate.
fn stand_ins(
    by_piece: &HashMap<PieceKey, String>,
    index: &BoostIndex,
    catalog: &EnhancementCatalog,
) -> HashMap<PieceKey, PieceKey> {
    let mut stand_ins = HashMap::new();
    for (name, entry) in &index.entries {
        let BoostEntry::Special { family, id } = entry else {
            continue;
        };
        let key = PieceKey::Special {
            category: family.clone(),
            id: id.clone(),
        };
        let Some(own) = special_def(catalog, family, id) else {
            continue;
        };
        if by_piece.contains_key(&key) {
            continue;
        }
        let name = name.to_lowercase();
        let mut candidates = by_piece.iter().filter(|(candidate, uid)| {
            let PieceKey::Special { category, id } = candidate else {
                return false;
            };
            name.ends_with(&format!("_{}", uid.to_lowercase()))
                && special_def(catalog, category, id).is_some_and(|def| same_aspects(def, own))
        });
        if let (Some((stand_in, _)), None) = (candidates.next(), candidates.next()) {
            stand_ins.insert(key, stand_in.clone());
        }
    }
    stand_ins
}

fn special_def<'a>(
    catalog: &'a EnhancementCatalog,
    category: &str,
    id: &str,
) -> Option<&'a SpecialEnhancementDef> {
    let (_, _, family) = catalog
        .special_families()
        .into_iter()
        .find(|(_, tag, _)| *tag == category)?;
    family.get(id)
}

fn same_aspects(a: &SpecialEnhancementDef, b: &SpecialEnhancementDef) -> bool {
    let sorted = |def: &SpecialEnhancementDef| {
        let mut aspects: Vec<AspectValue> = def.aspects.clone();
        aspects.sort_by(|x, y| x.stat.cmp(&y.stat).then(x.value.total_cmp(&y.value)));
        aspects
    };
    sorted(a) == sorted(b)
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

// ============================================================
// The build.
// ============================================================

/// Something the file can't say the way the build does, for the player to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportNote {
    /// What it's about: a power, a slot, or the build.
    pub context: String,
    pub detail: String,
}

/// A build written for Mids, and what didn't make it across intact.
#[derive(Debug, Clone, PartialEq)]
pub struct MidsExport {
    pub file: MbdFile,
    pub notes: Vec<ExportNote>,
}

impl MidsExport {
    /// The file as Mids writes it, with Mids' `PascalCase` keys.
    pub fn text(&self) -> String {
        let value = serde_json::to_value(&self.file).expect("an MbdFile is plain data");
        serde_json::to_string_pretty(&pascal_keys(value)).expect("a Value always serializes")
    }
}

/// Why nothing could be written at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("this dataset ships no {0}, so a Mids file can't be written")]
    Missing(&'static str),
    #[error("{0}")]
    Table(String),
}

/// The app that wrote the file, for Mids' `BuiltWith`. Provenance only; nothing reads it.
const APP: &str = "CoH Sidekick";

/// Mids' seven `Inherent` grid rows. Mids files each at a fixed index and sizes the array by
/// how many the file holds, so leaving one out throws inside its load. A fact about Mids.
const MIDS_INHERENT_GRID: [&str; 7] = [
    "Brawl", "Sprint", "Rest", "Swift", "Hurdle", "Health", "Stamina",
];

/// The pool positions in `PowerSets`, between the reserved third entry and the epic. Every
/// file Mids wrote has eight entries.
const MIDS_POOL_POSITIONS: usize = 4;

/// Write `state` as a Mids `.mbd`.
///
/// `default_io_level` is the level a crafted piece that states none is written at.
pub fn write_mbd(
    state: &CharacterState,
    database: &PowerDatabase,
    default_io_level: Level,
) -> Result<MidsExport, ExportError> {
    let names = database
        .mids_names
        .as_ref()
        .ok_or(ExportError::Missing("Mids name map"))?;
    let schedule = database
        .leveling_schedule
        .as_ref()
        .ok_or(ExportError::Missing("leveling schedule"))?;
    let ids = MidsEnhancementIds::new(
        database
            .mids_uids
            .as_ref()
            .ok_or(ExportError::Missing("Mids enhancement table"))?,
        database
            .boost_index
            .as_ref()
            .ok_or(ExportError::Missing("boost index"))?,
        database
            .enhancements
            .as_ref()
            .ok_or(ExportError::Missing("enhancement catalog"))?,
    )
    .map_err(ExportError::Table)?;
    let grants = database
        .inherent_grants()
        .map_err(ExportError::Table)?
        .ok_or(ExportError::Missing("inherent list"))?;

    let mut writer = Writer {
        state,
        database,
        names,
        ids: &ids,
        levels: slot_levels(schedule, state),
        default_io_level,
        notes: Vec::new(),
    };

    let mids_database = writer.mids_database();
    let class = writer.class();
    let alignment = writer.alignment();
    let power_sets = writer.power_sets();
    let picks = writer.picks(schedule);
    let last_power = i32::try_from(picks.len()).unwrap_or(i32::MAX);
    let mut power_entries = picks;
    power_entries.extend(writer.inherent_grid(&grants));
    power_entries.extend(writer.archetype_inherent(&class));
    power_entries.extend(writer.granted_powers());
    power_entries.extend(writer.incarnates());
    power_entries.extend(writer.accolades());
    writer.packed_levels_note();

    let mut file = MbdFile {
        built_with: MbdBuiltWith {
            app: APP.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            database: mids_database.to_string(),
            database_version: String::new(),
        },
        level: String::new(),
        class,
        origin: state.origin.clone().unwrap_or_default(),
        alignment,
        name: state.name.clone(),
        comment: String::new(),
        power_sets,
        last_power,
        power_entries,
    };
    // Mids recomputes this on load and stores it 0-based; stated the same way so it matches.
    file.level = (i32::from(mids_max_used_level(&file)) - 1)
        .max(0)
        .to_string();

    Ok(MidsExport {
        file,
        notes: writer.notes,
    })
}

struct Writer<'a> {
    state: &'a CharacterState,
    database: &'a PowerDatabase,
    names: &'a MidsNames,
    ids: &'a MidsEnhancementIds,
    levels: SlotLevels,
    default_io_level: Level,
    notes: Vec<ExportNote>,
}

impl<'a> Writer<'a> {
    fn note(&mut self, context: impl Into<String>, detail: impl Into<String>) {
        self.notes.push(ExportNote {
            context: context.into(),
            detail: detail.into(),
        });
    }

    /// Mids' database for this fork, with a note where Mids has none of its own (MBDEXPORT-2).
    fn mids_database(&mut self) -> &'static str {
        let dataset = self.state.dataset;
        let database = mids_database_for(dataset);
        if dataset_for_database(database) != Some(dataset) {
            self.note(
                dataset.display_name(),
                format!(
                    "Mids has no {} database, so this file is written for its {database} one. \
                     Powers and enhancements only {} has may not appear in Mids.",
                    dataset.display_name(),
                    dataset.display_name()
                ),
            );
        }
        database
    }

    /// The archetype's own class token, read off the dataset (MBDEXPORT-6's Guardian).
    fn class(&mut self) -> String {
        let class = self
            .state
            .archetype
            .id
            .as_deref()
            .and_then(|id| self.database.class_name_of(id));
        match class {
            Some(class) => class.to_string(),
            None => {
                self.note("archetype", "this build has no archetype Mids can name");
                String::new()
            }
        }
    }

    /// Sidekick doesn't keep an alignment, so the archetype's starting side stands in for it.
    fn alignment(&self) -> String {
        let side = self
            .state
            .archetype
            .id
            .as_deref()
            .and_then(|id| {
                self.database
                    .archetypes()
                    .ok()?
                    .get(id)
                    .map(|a| a.side.clone())
            })
            .unwrap_or_default();
        let mut letters = side.chars();
        match letters.next() {
            Some(first) => first.to_uppercase().chain(letters).collect(),
            None => String::new(),
        }
    }

    /// The game's own path for one of our sets: `Blaster_Ranged.Assault_Rifle`.
    fn game_path(&self, set_id: &str) -> Option<&'a str> {
        if let Some(set) = self.database.find_powerset(set_id) {
            return set.set_path.as_deref();
        }
        let catalog = &self.database.pool_catalog;
        catalog
            .pools
            .iter()
            .chain(catalog.epics.iter())
            .find(|pool| pool.id == set_id)?
            .set_path
            .as_deref()
    }

    /// The path Mids writes for a game path (MBDEXPORT-6, -9).
    fn mids_path(&self, game_path: &str) -> String {
        self.names
            .mids_powerset_path(&fold_path(game_path))
            .map_or_else(|| game_path.to_string(), str::to_string)
    }

    /// `Group.Set.Power` as Mids spells it (MBDEXPORT-3).
    fn mids_full_name(&self, game_path: &str, power: &str) -> String {
        let name = self
            .names
            .mids_power_name(&fold_path(game_path), power)
            .unwrap_or(power);
        format!("{}.{name}", self.mids_path(game_path))
    }

    fn set_mids_path(&mut self, set_id: Option<&str>) -> String {
        let Some(set_id) = set_id else {
            return String::new();
        };
        match self.game_path(set_id) {
            Some(path) => self.mids_path(path),
            None => {
                self.note(
                    set_id,
                    "this set has no game path, so Mids gets no entry for it",
                );
                String::new()
            }
        }
    }

    /// Primary, secondary, a reserved blank, four pools, the epic.
    fn power_sets(&mut self) -> Vec<String> {
        let state = self.state;
        let primary = header_set(state.primary.id.as_deref(), &state.primary.powers);
        let secondary = header_set(state.secondary.id.as_deref(), &state.secondary.powers);
        let mut sets = vec![
            self.set_mids_path(primary),
            self.set_mids_path(secondary),
            String::new(),
        ];
        let mut pools: Vec<String> = state
            .pools
            .iter()
            .map(|pool| self.set_mids_path(Some(&pool.id)))
            .collect();
        pools.resize(MIDS_POOL_POSITIONS.max(pools.len()), String::new());
        sets.extend(pools);
        sets.push(self.set_mids_path(state.epic_pool.as_ref().map(|epic| epic.id.as_str())));
        sets
    }

    /// The picked powers along the pick schedule, a blank row for each pick not taken. Mids
    /// reads this run by position, so row i is the i-th level-up pick.
    fn picks(&mut self, schedule: &LevelingSchedule) -> Vec<MbdPowerEntry> {
        let state = self.state;
        let mut picked: Vec<&SelectedPower> = state
            .primary
            .powers
            .iter()
            .chain(state.secondary.powers.iter())
            .chain(state.pools.iter().flat_map(|pool| pool.powers.iter()))
            .chain(state.epic_pool.iter().flat_map(|epic| epic.powers.iter()))
            .filter(|power| power.level > 0)
            .collect();
        picked.sort_by_key(|power| power.level);

        let mut entries = Vec::new();
        let mut picked = picked.into_iter().peekable();
        for level in schedule.pick_levels() {
            match picked.next_if(|power| power.level <= level) {
                Some(power) => {
                    let entry = self.pick_entry(power);
                    entries.push(entry);
                }
                None => entries.push(blank_entry()),
            }
        }
        for power in picked {
            self.note(
                power.internal_name.as_str(),
                format!(
                    "taken at level {}, past the last pick this server's schedule grants, so \
                     Mids gets it in the last row",
                    power.level
                ),
            );
            let entry = self.pick_entry(power);
            entries.push(entry);
        }
        entries
    }

    fn pick_entry(&mut self, power: &SelectedPower) -> MbdPowerEntry {
        let name = match self.game_path(&power.powerset) {
            Some(path) => self.mids_full_name(path, &power.internal_name),
            None => {
                self.note(
                    power.internal_name.as_str(),
                    "its set has no game path, so Mids gets the bare name",
                );
                power.internal_name.clone()
            }
        };
        self.power_entry(power, name, i32::from(power.level))
    }

    fn power_entry(&mut self, power: &SelectedPower, name: String, level: i32) -> MbdPowerEntry {
        let slot_entries = self.slot_entries(power, level);
        MbdPowerEntry {
            power_name: name,
            level,
            // Only `true` writes `true` (MBDEXPORT-13).
            stat_include: power.is_active,
            proc_include: false,
            variable_value: power
                .targets_hit
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or(0),
            inherent_slots_used: i32::from(power.inherent_slot_count),
            sub_power_entries: Value::Array(Vec::new()),
            slot_entries,
        }
    }

    fn slot_entries(&mut self, power: &SelectedPower, pick_level: i32) -> Vec<MbdSlotEntry> {
        let solved: Vec<Option<u8>> = levels_of(&self.levels, self.state, power)
            .map(<[Option<u8>]>::to_vec)
            .unwrap_or_default();
        let category = SlotCategory::of(self.state, &power.powerset, &power.internal_name);
        let auto_from = power
            .slots
            .len()
            .saturating_sub(usize::from(power.inherent_slot_count));
        let mut entries = Vec::with_capacity(power.slots.len());
        for (index, slot) in power.slots.iter().enumerate() {
            // A placed level goes out as placed, even where this server's schedule would move
            // it; anything else goes out where the planner solves it (MBDEXPORT-21).
            let level = match index {
                0 => pick_level,
                _ => self
                    .placed_level(power, category, index)
                    .or_else(|| solved.get(index).copied().flatten())
                    .map_or(pick_level, i32::from),
            };
            let enhancement = slot
                .as_ref()
                .and_then(|enhancement| self.piece(power, index, enhancement));
            entries.push(MbdSlotEntry {
                level: level.max(1),
                is_inherent: index > 0 && index >= auto_from,
                enhancement,
                flipped_enhancement: Value::Null,
            });
        }
        entries
    }

    fn placed_level(
        &self,
        power: &SelectedPower,
        category: Option<SlotCategory>,
        index: usize,
    ) -> Option<u8> {
        self.state
            .slot_order
            .iter()
            .find(|entry| {
                entry.power_name == power.internal_name
                    && usize::from(entry.slot_index) == index
                    && entry.level_source == Some(SlotLevelSource::Authored)
                    && entry
                        .category
                        .as_deref()
                        .is_none_or(|stated| Some(stated) == category.map(SlotCategory::as_str))
            })?
            .level
    }

    fn piece(
        &mut self,
        power: &SelectedPower,
        index: usize,
        enhancement: &crate::character::Enhancement,
    ) -> Option<MbdEnhancement> {
        let context = format!("{}, slot {}", power.internal_name, index + 1);
        let skif = match slim_enhancement(enhancement) {
            Ok(skif) => skif,
            Err(error) => {
                self.note(context, format!("{error}; the slot goes out empty"));
                return None;
            }
        };
        match self.ids.mids_uid_for(&skif, self.default_io_level) {
            Ok(piece) => {
                for loss in &piece.losses {
                    self.note(context.as_str(), loss_text(loss));
                }
                Some(piece.enhancement)
            }
            Err(refusal) => {
                self.note(context, format!("{refusal}; the slot goes out empty"));
                None
            }
        }
    }

    /// Mids' seven grid inherents, each with what the build put on it, or a bare row.
    fn inherent_grid(&mut self, grants: &InherentGrants) -> Vec<MbdPowerEntry> {
        let mut entries = Vec::new();
        for name in MIDS_INHERENT_GRID {
            let grant = grants
                .grants
                .iter()
                .find(|grant| grant.internal_name == name);
            let level = grant
                .and_then(|grant| grant.granted_at_level)
                .map_or(1, i32::from);
            let game = grant
                .and_then(|grant| grant.full_name.as_deref())
                .and_then(|full| full.rsplit_once('.'));
            let full_name = match game {
                Some((path, power)) => self.mids_full_name(path, power),
                None => {
                    // The row still has to be there for Mids to load the file.
                    self.note(
                        name,
                        "this server's inherent list doesn't name it; Mids gets a bare row",
                    );
                    let path = self
                        .game_path(crate::INHERENT_SET)
                        .unwrap_or(crate::INHERENT_SET);
                    format!("{}.{name}", self.mids_path(path))
                }
            };
            let held = self
                .state
                .inherents
                .iter()
                .find(|power| power.internal_name == name);
            let entry = match held {
                Some(power) => self.power_entry(power, full_name, level),
                None => MbdPowerEntry {
                    power_name: full_name,
                    level,
                    slot_entries: vec![MbdSlotEntry {
                        level,
                        is_inherent: false,
                        enhancement: None,
                        flipped_enhancement: Value::Null,
                    }],
                    ..blank_entry()
                },
            };
            entries.push(entry);
        }
        entries
    }

    /// The archetype's own inherent, under the name Mids shows for the class (MBDEXPORT-20, -24).
    fn archetype_inherent(&mut self, class: &str) -> Option<MbdPowerEntry> {
        let state = self.state;
        let declared = self
            .database
            .archetypes()
            .ok()?
            .get(state.archetype.id.as_deref()?)?
            .inherent
            .name
            .clone();
        let ident = self
            .database
            .archetype_inherent(&declared)?
            .ident()
            .to_string();
        let path = self.game_path(crate::INHERENT_SET)?;
        let name = match self.names.archetype_inherent(class) {
            Some(name) => format!("{}.{name}", self.mids_path(path)),
            None => {
                self.note(
                    declared.as_str(),
                    format!("Mids shows no inherent for {class}, so it gets ours and may skip it"),
                );
                self.mids_full_name(path, &ident)
            }
        };
        let entry = match state
            .inherents
            .iter()
            .find(|power| power.internal_name == ident)
        {
            Some(power) => self.power_entry(power, name, 1),
            None => MbdPowerEntry {
                power_name: name,
                level: 1,
                stat_include: true,
                ..blank_entry()
            },
        };
        Some(entry)
    }

    /// Powers the game hands over for a pick, such as a Kheldian form's attacks (MBDEXPORT-5).
    /// Each one's `requires` names both the pick that grants it and, negated, its own path —
    /// `Inherent.Inherent.Dark_Nova_Blast`, which is where Mids files it too. One with no slots
    /// carries nothing, and Mids re-creates it, so it isn't written.
    fn granted_powers(&mut self) -> Vec<MbdPowerEntry> {
        let state = self.state;
        let fallback_path = self.game_path(crate::INHERENT_SET);
        let picked = || {
            state
                .primary
                .powers
                .iter()
                .chain(state.secondary.powers.iter())
                .chain(state.pools.iter().flat_map(|pool| pool.powers.iter()))
                .chain(state.epic_pool.iter().flat_map(|epic| epic.powers.iter()))
        };
        let mut entries = Vec::new();
        for power in picked().filter(|power| power.level == 0 && !power.slots.is_empty()) {
            let def = self
                .database
                .find_power(&power.powerset, &power.internal_name)
                .or_else(|| {
                    self.database
                        .find_partition_power(&power.powerset, &power.internal_name)
                });
            let gate = def.map(requirement_paths).unwrap_or_default();
            let granted_by_pick = |parent: &SelectedPower| {
                gate.iter().any(|(path, negated)| {
                    !negated && path.rsplit('.').next() == Some(parent.internal_name.as_str())
                })
            };
            let level = picked()
                .filter(|parent| parent.level > 0 && granted_by_pick(parent))
                .map(|parent| parent.level)
                .max()
                .or_else(|| def.map(|def| def.unlock_level()))
                .unwrap_or(1);
            let own_path = gate.iter().find_map(|(path, negated)| {
                let (set, name) = path.rsplit_once('.')?;
                (*negated && name == power.internal_name).then_some(set)
            });
            let Some(path) = own_path.or(fallback_path) else {
                continue;
            };
            let name = self.mids_full_name(path, &power.internal_name);
            entries.push(self.power_entry(power, name, i32::from(level)));
        }
        entries
    }

    fn incarnates(&mut self) -> Vec<MbdPowerEntry> {
        let catalog = &self.database.incarnate_catalog;
        let mut entries = Vec::new();
        for (slot_id, pick) in self.state.incarnates.occupied() {
            let found = catalog
                .slot(slot_id)
                .and_then(|slot| Some((slot, slot.find_power(&pick.power_name)?)));
            let Some((slot, power)) = found else {
                self.note(
                    pick.power_name.as_str(),
                    format!("no {slot_id} power of that name, so Mids gets no entry for it"),
                );
                continue;
            };
            entries.push(MbdPowerEntry {
                power_name: self.mids_full_name(&slot.key, &power.internal_name),
                level: i32::from(self.state.level.max(1)),
                stat_include: pick.active,
                ..blank_entry()
            });
        }
        entries
    }

    /// Mids files accolades as temporary powers (MBDEXPORT-12). Only switched-on ones are held.
    fn accolades(&mut self) -> Vec<MbdPowerEntry> {
        let toggles = self.database.accolade_toggles();
        let mut entries = Vec::new();
        for id in &self.state.accolades {
            let Some(toggle) = toggles.iter().find(|toggle| &toggle.id == id) else {
                self.note(
                    id.as_str(),
                    "no accolade by that name, so Mids gets no entry for it",
                );
                continue;
            };
            let path = self
                .database
                .accolade_powers_with_set()
                .find(|(_, power)| std::ptr::eq(*power, toggle.power))
                .and_then(|(set_id, _)| self.game_path(set_id));
            let Some(path) = path else {
                self.note(
                    id.as_str(),
                    "its set has no game path, so Mids gets no entry for it",
                );
                continue;
            };
            entries.push(MbdPowerEntry {
                power_name: self.mids_full_name(path, toggle.power.ident()),
                level: i32::from(self.state.level.max(1)),
                stat_include: true,
                ..blank_entry()
            });
        }
        entries
    }

    /// One note when some slot levels came from the planner's packing rather than a placement
    /// (MBDEXPORT-21). Mids needs a level on every slot, so the file states a legal order.
    fn packed_levels_note(&mut self) {
        let authored = |power: &SelectedPower, index: usize| {
            self.state.slot_order.iter().any(|entry| {
                entry.power_name == power.internal_name
                    && usize::from(entry.slot_index) == index
                    && entry.level_source == Some(SlotLevelSource::Authored)
            })
        };
        let packed = self.state.all_selected().any(|power| {
            let user = 1..power
                .slots
                .len()
                .saturating_sub(usize::from(power.inherent_slot_count));
            user.into_iter().any(|index| !authored(power, index))
        });
        if packed {
            self.note(
                "slot levels",
                "some slot levels were filled in by the planner rather than placed one at a \
                 time, so the file states a legal order, not how the character was levelled",
            );
        }
    }
}

/// Every power path a `requires` names, and whether it's negated (`<path> !`).
fn requirement_paths(def: &crate::Power) -> Vec<(String, bool)> {
    let tokens = requires(def).unwrap_or_default();
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.contains('.'))
        .map(|(index, token)| {
            let negated = tokens.get(index + 1).is_some_and(|next| &**next == "!");
            (token.to_string(), negated)
        })
        .collect()
}

/// The set a role's header names: the branch set once an Arachnos build has specialised into
/// one, which is how Mids writes it (MBDEXPORT-16). Picks keep their own sets either way.
fn header_set<'s>(selection: Option<&'s str>, powers: &'s [SelectedPower]) -> Option<&'s str> {
    let mut others = powers
        .iter()
        .map(|power| power.powerset.as_str())
        .filter(|set| Some(*set) != selection);
    let first = others.next();
    match first {
        Some(branch) if others.all(|set| set == branch) => Some(branch),
        _ => selection,
    }
}

/// Mids' database name for each fork. Two forks have no database of their own (MBDEXPORT-2):
/// Brainstorm builds on Homecoming's, and Mids 3.8.6 dies on `Thunderspy`, so it gets Generic.
fn mids_database_for(dataset: DatasetId) -> &'static str {
    match dataset {
        DatasetId::Homecoming | DatasetId::Brainstorm => "Homecoming",
        DatasetId::Rebirth => "Rebirth",
        DatasetId::Thunderspy => "Generic",
    }
}

/// An unfilled pick, as Mids writes one.
fn blank_entry() -> MbdPowerEntry {
    MbdPowerEntry {
        power_name: String::new(),
        level: -1,
        stat_include: false,
        proc_include: false,
        variable_value: 0,
        inherent_slots_used: 0,
        sub_power_entries: Value::Array(Vec::new()),
        slot_entries: Vec::new(),
    }
}

fn loss_text(loss: &PieceLoss) -> String {
    match loss {
        PieceLoss::Attunement {
            written_attuned: false,
        } => "Mids only has the crafted form of this piece, so it goes out crafted".to_string(),
        PieceLoss::Attunement {
            written_attuned: true,
        } => "Mids only has the attuned form of this piece, so it goes out attuned".to_string(),
        PieceLoss::LevelClamped { asked, kept } => format!(
            "crafted at level {asked}, which Mids doesn't allow for it; Mids will show it at {kept}"
        ),
        PieceLoss::RelativeLevelClamped { asked, kept } => {
            format!("{asked:+} levels is outside what Mids can store; it goes out at {kept:+}")
        }
        PieceLoss::SwappedFor { category, id } => format!(
            "Mids has no record of this enhancement, so it goes out as the {category} {id}, \
             which does the same thing"
        ),
    }
}

/// Mids' keys are `PascalCase`; the typed file is `snake_case`.
fn pascal_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (snake_to_pascal(&key), pascal_keys(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(pascal_keys).collect()),
        other => other,
    }
}

fn snake_to_pascal(key: &str) -> String {
    key.split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}
