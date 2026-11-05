//! A VEAT read from the game keeps each branch-set power under its OWN set.
//!
//! Reported by a user 2026-11-05 about Homecoming's new Vault link: "issues loading in VEAT
//! builds (will import, but it won't recognize the power names of several things so you end up
//! with borked icons and internal names)".
//!
//! The build does import. The client hands over one flat run of `Widow_Training.*` lines with
//! the base set's and the branch's powers interleaved in purchase order, and the reader seated
//! the primary list at whichever line arrived first, then filed every other pick under that
//! seat — `game_import::push` was handed the pick's resolved set id and dropped it on an already
//! open selection. `skif::hydrate` widens its NAME lookup across the archetype's branch sets
//! (so the ident itself still resolves), but it keeps the seated set as the pick's own, and the
//! app renders a power through `view::power_view::resolve_power_def`, an exact
//! `find_power(powerset, ident)`. A base seat paired with a branch ident names nothing, and
//! Rule 1's fallback is what the user saw: the internal name beside the unknown icon.
//!
//! Only a VEAT can show this, because only a VEAT has picks whose real set differs from the
//! seat — which is why the whole `.mbd`/`.mxd`/`/buildsave`/Vault corpus reads clean with the
//! fix and read clean without it. `crate::mbd_import`'s `push` has stamped the pick since the
//! base-seat fix; the game reader is the one that lagged.
//!
//! Both orders are graded, because the seat is whichever pick arrives first: base-first leaves
//! the branch picks to be stamped, branch-first seats the branch and leans on
//! `skif::seat_base_sets` to move the seat while the base picks keep their own stamp.

use coh_data::{
    client_build, game_export, game_import, skif, CharacterState, DatasetId, PowerDatabase,
};
use std::path::PathBuf;

fn homecoming() -> PowerDatabase {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract")
        .join(DatasetId::Homecoming.as_str())
        .join("bundle.json.gz");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
}

/// A Night Widow's two lists, in the client's spelling: every token lower-cased, the branch's
/// powers interleaved with the base set's in the order a level-50 would have bought them.
///
/// Legal, not merely plausible: one pick per level in each column, each at or above the level
/// the dataset says the power becomes available. This is the same build the in-app check loads
/// off a Vault-style link, so the two grade one character.
const NIGHT_WIDOW_CLIENT_JSON: &str = r#"{
  "build": {
    "character": { "name": "Night Widow", "archetype": "class_arachnos_widow", "origin": "military", "level": 50 },
    "powers": [
      { "categoryName": "widow_training", "powerSetName": "widow_training", "powerName": "poison_dart", "powerLevelBought": 1, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "widow_training", "powerName": "lunge", "powerLevelBought": 2, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "night_widow_training", "powerName": "nw_mental_blast", "powerLevelBought": 7, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "night_widow_training", "powerName": "nw_smoke_grenade", "powerLevelBought": 11, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "widow_training", "powerName": "dart_burst", "powerLevelBought": 14, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "widow_training", "powerName": "confront", "powerLevelBought": 17, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "night_widow_training", "powerName": "nw_slash", "powerLevelBought": 18, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "night_widow_training", "powerName": "nw_eviscerate", "powerLevelBought": 21, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "widow_training", "powerSetName": "night_widow_training", "powerName": "nw_psychic_scream", "powerLevelBought": 23, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "teamwork", "powerName": "combat_training:_defensive", "powerLevelBought": 1, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "teamwork", "powerName": "tactical_training:_maneuvers", "powerLevelBought": 3, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "widow_teamwork", "powerName": "pain_tolerance", "powerLevelBought": 5, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "teamwork", "powerName": "indomitable_will", "powerLevelBought": 9, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "widow_teamwork", "powerName": "nw_mask_presence", "powerLevelBought": 19, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "teamwork", "powerName": "foresight", "powerLevelBought": 21, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "widow_teamwork", "powerName": "nw_mind_link", "powerLevelBought": 23, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "widow_teamwork", "powerName": "nw_tactical_training:_vengeance", "powerLevelBought": 27, "powerNumBoostsBought": 0, "boosts": [] },
      { "categoryName": "teamwork", "powerSetName": "widow_teamwork", "powerName": "elude", "powerLevelBought": 29, "powerNumBoostsBought": 0, "boosts": [] }
    ]
  }
}"#;

/// The same build in `/buildsave`'s text, which shares `game_import::to_skif_build` with the
/// client JSON and reaches it through a different reader.
const NIGHT_WIDOW_BUILDSAVE: &str = "\
Night Widow: Level 50 Military Class_Arachnos_Widow

Character Profile:
------------------
Level 1: Widow_Training Widow_Training Poison_Dart
\tEMPTY
Level 2: Widow_Training Widow_Training Lunge
\tEMPTY
Level 7: Widow_Training Night_Widow_Training NW_Mental_Blast
\tEMPTY
Level 11: Widow_Training Night_Widow_Training NW_Smoke_Grenade
\tEMPTY
Level 14: Widow_Training Widow_Training Dart_Burst
\tEMPTY
Level 17: Widow_Training Widow_Training Confront
\tEMPTY
Level 18: Widow_Training Night_Widow_Training NW_Slash
\tEMPTY
Level 21: Widow_Training Night_Widow_Training NW_Eviscerate
\tEMPTY
Level 23: Widow_Training Night_Widow_Training NW_Psychic_Scream
\tEMPTY
Level 1: Teamwork Teamwork Combat_Training:_Defensive
\tEMPTY
Level 3: Teamwork Teamwork Tactical_Training:_Maneuvers
\tEMPTY
Level 5: Teamwork Widow_Teamwork Pain_Tolerance
\tEMPTY
Level 9: Teamwork Teamwork Indomitable_Will
\tEMPTY
Level 19: Teamwork Widow_Teamwork NW_Mask_Presence
\tEMPTY
Level 21: Teamwork Teamwork Foresight
\tEMPTY
Level 23: Teamwork Widow_Teamwork NW_Mind_Link
\tEMPTY
Level 27: Teamwork Widow_Teamwork NW_Tactical_Training:_Vengeance
\tEMPTY
Level 29: Teamwork Widow_Teamwork Elude
\tEMPTY
";

/// The same build with its two opening base lines gone, so a branch line leads the primary list
/// and the selection seats on the BRANCH. Whole lines out of the middle of the list, so the
/// document stays valid JSON.
fn branch_first() -> String {
    NIGHT_WIDOW_CLIENT_JSON
        .lines()
        .filter(|line| {
            !line.contains(r#""powerName": "poison_dart""#)
                && !line.contains(r#""powerName": "lunge""#)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The app's own door: `view::power_view::resolve_power_def` for a powerset pick is an exact
/// `find_power(powerset, ident)`, and a miss is what draws the internal name and the unknown
/// icon. This is the pair the user's eyes land on, so it is the pair the test grades.
fn unresolvable<'a>(build: &'a CharacterState, db: &'a PowerDatabase) -> Vec<String> {
    build
        .primary
        .powers
        .iter()
        .chain(build.secondary.powers.iter())
        .filter(|pick| db.find_power(&pick.powerset, &pick.internal_name).is_none())
        .map(|pick| format!("{} :: {}", pick.powerset, pick.internal_name))
        .collect()
}

fn display(build: &CharacterState, db: &PowerDatabase, ident: &str) -> String {
    let pick = build
        .all_selected()
        .find(|p| p.internal_name == ident)
        .unwrap_or_else(|| panic!("{ident} is in the build"));
    db.find_power(&pick.powerset, ident)
        .unwrap_or_else(|| panic!("{ident} resolves"))
        .name
        .clone()
}

fn hydrate(build: skif::SkifBuild, db: &PowerDatabase) -> CharacterState {
    let decoded = skif::hydrate(
        skif::SkifFile {
            version: skif::VERSION,
            authored_against: None,
            meta: None,
            build,
        },
        db,
    )
    .expect("the build hydrates");
    assert!(
        decoded.unresolved.is_empty(),
        "nothing was left unresolved: {:?}",
        decoded.unresolved
    );
    decoded.build
}

fn from_client_json(text: &str, db: &PowerDatabase) -> CharacterState {
    let document = client_build::parse(text).expect("the client document parses");
    let read = client_build::read(&document, db.boost_index.as_ref()).expect("the build reads");
    hydrate(
        game_import::to_skif_build(&read.export, db, DatasetId::Homecoming).build,
        db,
    )
}

/// The base pair in the seats, the branch's powers under the branch, and every pick a
/// definition can be found for.
fn assert_widow(build: &CharacterState, db: &PowerDatabase) {
    assert_eq!(
        build.primary.id.as_deref(),
        Some("arachnos-widow/widow-training"),
        "the base set holds the primary seat"
    );
    assert_eq!(
        build.secondary.id.as_deref(),
        Some("arachnos-widow/teamwork"),
        "the base set holds the secondary seat"
    );
    let orphans = unresolvable(build, db);
    assert!(
        orphans.is_empty(),
        "every pick names a definition: {orphans:?}"
    );
    for (ident, set) in [
        ("NW_Mental_Blast", "arachnos-widow/night-widow-training"),
        ("NW_Smoke_Grenade", "arachnos-widow/night-widow-training"),
        ("NW_Slash", "arachnos-widow/night-widow-training"),
        ("NW_Eviscerate", "arachnos-widow/night-widow-training"),
        ("NW_Psychic_Scream", "arachnos-widow/night-widow-training"),
        ("NW_Mask_Presence", "arachnos-widow/widow-teamwork"),
        ("NW_Mind_Link", "arachnos-widow/widow-teamwork"),
        (
            "NW_Tactical_Training:_Vengeance",
            "arachnos-widow/widow-teamwork",
        ),
        ("Poison_Dart", "arachnos-widow/widow-training"),
        ("Dart_Burst", "arachnos-widow/widow-training"),
        ("Confront", "arachnos-widow/widow-training"),
        ("Indomitable_Will", "arachnos-widow/teamwork"),
        ("Foresight", "arachnos-widow/teamwork"),
        ("Pain_Tolerance", "arachnos-widow/widow-teamwork"),
        ("Elude", "arachnos-widow/widow-teamwork"),
    ] {
        let pick = build
            .all_selected()
            .find(|p| p.internal_name == ident)
            .unwrap_or_else(|| panic!("{ident} is in the build"));
        assert_eq!(pick.powerset, set, "{ident} sits under its own set");
    }
    // The display names the report said were missing. `Mental Blast` and `Slash` are the
    // branch's own; `Lunge` is the set's renamed pair, where the binary ident and the display
    // name point at each other.
    assert_eq!(display(build, db, "NW_Mental_Blast"), "Mental Blast");
    assert_eq!(display(build, db, "NW_Slash"), "Slash");
    assert_eq!(display(build, db, "NW_Mask_Presence"), "Mask Presence");
    assert_eq!(display(build, db, "Lunge"), "Strike");
}

#[test]
fn a_vault_veat_build_keeps_its_branch_powers_under_their_own_set() {
    let db = homecoming();
    assert_widow(&from_client_json(NIGHT_WIDOW_CLIENT_JSON, &db), &db);
}

#[test]
fn a_buildsave_veat_build_keeps_them_too() {
    let db = homecoming();
    let export = game_export::parse(NIGHT_WIDOW_BUILDSAVE).expect("the export parses");
    let build = game_import::to_skif_build(&export, &db, DatasetId::Homecoming);
    assert!(
        build.unresolved.is_empty(),
        "the text reader refused nothing: {:?}",
        build.unresolved
    );
    assert_widow(&hydrate(build.build, &db), &db);
}

/// The branch line first seats the branch, and `seat_base_sets` moves the seat back — the base
/// picks survive that move because they were stamped with their own set on the way in.
#[test]
fn a_branch_first_vault_build_seats_the_base_set_and_keeps_both() {
    let db = homecoming();
    let build = from_client_json(&branch_first(), &db);
    assert_eq!(
        build.primary.id.as_deref(),
        Some("arachnos-widow/widow-training"),
        "the seat moved off the branch"
    );
    let orphans = unresolvable(&build, &db);
    assert!(
        orphans.is_empty(),
        "every pick names a definition: {orphans:?}"
    );
    // The two opening base lines were dropped so a branch line could lead. `Confront` is the
    // base pick that arrives AFTER them, and it must sit under the base set rather than under
    // the branch seat it was merged into.
    let confront = build
        .all_selected()
        .find(|p| p.internal_name == "Confront")
        .expect("the base pick survived");
    assert_eq!(
        confront.powerset, "arachnos-widow/widow-training",
        "Confront did not stay with the branch seat"
    );
}

/// A non-VEAT build is untouched by the stamp: every pick belongs to the list it sits in, so
/// none of them states a set of its own.
#[test]
fn an_ordinary_build_states_no_set_on_any_pick() {
    let db = homecoming();
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/buildsave/homecoming/charnel.txt"),
    )
    .expect("the fixture reads");
    let export = game_export::parse(&text).expect("the export parses");
    let build = game_import::to_skif_build(&export, &db, DatasetId::Homecoming);
    for pick in build
        .build
        .primary
        .iter()
        .flat_map(|s| &s.powers)
        .chain(build.build.secondary.iter().flat_map(|s| &s.powers))
    {
        assert_eq!(
            pick.powerset, None,
            "{} needs no set of its own",
            pick.internal_name
        );
    }
    let state = hydrate(build.build, &db);
    assert!(unresolvable(&state, &db).is_empty());
}
