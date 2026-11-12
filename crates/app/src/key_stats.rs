//! Key stats — the four numbers a phone keeps under the build's name while the build is made
//! (phone-build-flow-2). Each archetype starts with a default four, chosen for what that
//! archetype usually builds toward; the user can swap any of them from the Stats sheet, and the
//! swap is kept per archetype, so tuning a Tanker's four leaves a Blaster's alone.
//!
//! Every number is a [`stat_registry`] row read off the same [`BuildTotals`] the dashboard
//! reads, so the strip and the Stats sheet cannot disagree.
//!
//! A preference, not build state: it lives in localStorage, like [`crate::tracked_stats`].

use crate::build_session::BuildSession;
use crate::mobile_nav::OpenSheet;
use crate::panels::stat_registry::{self, StatDef, StatSection};
use crate::panels::stats::BuildTotals;
use dioxus::prelude::*;
use std::collections::BTreeMap;

const KEY_KEY_STATS: &str = "sk-key-stats";

/// How many numbers the strip holds. Four fit a 360px phone at a readable size.
pub const COUNT: usize = 4;

/// The default four per archetype id, as the datasets spell them. One table, so a fork's new
/// archetype is one line here, and the test below fails until it has one.
const DEFAULTS: &[(&str, [&str; COUNT])] = &[
    (
        "tanker",
        ["def_smashing", "res_smashing", "recharge", "health"],
    ),
    (
        "brute",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
    (
        "scrapper",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
    (
        "stalker",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
    (
        "guardian",
        ["def_smashing", "res_smashing", "recharge", "health"],
    ),
    (
        "sentinel",
        ["defense_ranged", "res_smashing", "recharge", "damage"],
    ),
    (
        "blaster",
        ["defense_ranged", "recharge", "damage", "recovery"],
    ),
    (
        "corruptor",
        ["defense_ranged", "recharge", "damage", "recovery"],
    ),
    (
        "defender",
        ["defense_ranged", "recharge", "recovery", "health"],
    ),
    (
        "controller",
        ["defense_ranged", "recharge", "recovery", "damage"],
    ),
    (
        "dominator",
        ["defense_ranged", "recharge", "recovery", "damage"],
    ),
    (
        "mastermind",
        ["defense_ranged", "recharge", "recovery", "health"],
    ),
    (
        "peacebringer",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
    (
        "warshade",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
    (
        "arachnos-soldier",
        ["defense_ranged", "defense_melee", "recharge", "damage"],
    ),
    (
        "arachnos-widow",
        ["defense_ranged", "defense_melee", "recharge", "damage"],
    ),
    (
        "primalist",
        ["def_smashing", "res_smashing", "recharge", "damage"],
    ),
];

/// Before an archetype is chosen, and for one the table has not met yet.
const FALLBACK: [&str; COUNT] = ["def_smashing", "res_smashing", "recharge", "health"];

fn defaults_for(archetype: Option<&str>) -> [&'static str; COUNT] {
    archetype
        .and_then(|id| DEFAULTS.iter().find(|(at, _)| *at == id))
        .map(|(_, four)| *four)
        .unwrap_or(FALLBACK)
}

/// The open build's archetype id, or `None` before one is chosen.
fn build_archetype() -> Option<String> {
    use_context::<BuildSession>()
        .build
        .read()
        .archetype
        .id
        .clone()
}

/// The user's swaps, by archetype id. An archetype with no entry shows its defaults.
#[derive(Clone, Copy)]
pub struct KeyStatChoices(pub Signal<BTreeMap<String, Vec<String>>>);

impl KeyStatChoices {
    /// The four rows to show for `archetype`: the user's choice where it is whole and still
    /// names real rows, the default otherwise.
    pub fn rows(&self, archetype: Option<&str>) -> [&'static StatDef; COUNT] {
        let chosen: Option<[&'static StatDef; COUNT]> = archetype
            .and_then(|id| self.0.read().get(id).cloned())
            .and_then(|ids| {
                let rows: Vec<&'static StatDef> = ids
                    .iter()
                    .filter_map(|id| stat_registry::by_id(id))
                    .collect();
                rows.try_into().ok()
            });
        chosen.unwrap_or_else(|| defaults_for(archetype).map(default_row))
    }

    /// Put `stat` in position `at` for `archetype`.
    pub fn set(mut self, archetype: &str, at: usize, stat: &str) {
        let mut ids: Vec<String> = self
            .rows(Some(archetype))
            .iter()
            .map(|row| row.id.to_string())
            .collect();
        if at < ids.len() {
            ids[at] = stat.to_string();
        }
        self.0.write().insert(archetype.to_string(), ids);
    }
}

/// A default id is checked by the test below, so a miss here is a typo the test reports first.
fn default_row(id: &'static str) -> &'static StatDef {
    stat_registry::by_id(id).unwrap_or(&stat_registry::ALL[0])
}

/// The strip's label: the row's own label, with its section's short name in front where the
/// label alone is ambiguous ("Smashing" is both a defense and a resistance).
pub fn short_label(stat: &StatDef) -> String {
    match stat.section {
        StatSection::Defense => format!("Def {}", stat.label),
        StatSection::Resistance => format!("Res {}", stat.label),
        _ => stat.label.to_string(),
    }
}

/// Provide [`KeyStatChoices`] at the shell root, restore it, and persist it on change.
pub fn use_key_stats_provider() {
    let choices = use_signal(BTreeMap::<String, Vec<String>>::new);
    use_context_provider(|| KeyStatChoices(choices));
    let mut restored = use_signal(|| false);
    use_future(move || async move {
        let mut choices = choices;
        if let Some(saved) = load().await {
            choices.set(saved);
        }
        restored.set(true);
    });
    use_effect(move || {
        let choices = choices.read();
        if *restored.read() {
            persist(&choices);
        }
    });
}

fn persist(choices: &BTreeMap<String, Vec<String>>) {
    let Ok(json) = serde_json::to_string(choices) else {
        return;
    };
    let js = format!("try {{ localStorage.setItem({KEY_KEY_STATS:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

async fn load() -> Option<BTreeMap<String, Vec<String>>> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_KEY_STATS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    serde_json::from_str(value.as_str()?).ok()
}

/// The strip: four numbers in a row, each in its stat's colour. Tapping one shows the Stats tab.
#[component]
pub fn KeyStatStrip() -> Element {
    let totals = use_context::<BuildTotals>().0;
    let choices = use_context::<KeyStatChoices>();
    let mut sheet = use_context::<OpenSheet>().0;
    let mut tab = use_context::<crate::phone_build::PhoneTab>().0;
    let archetype = build_archetype();
    let cells: Vec<(String, String, &'static str)> = {
        let totals = totals.read();
        choices
            .rows(archetype.as_deref())
            .iter()
            .map(|stat| {
                (
                    short_label(stat),
                    stat.format.render((stat.read)(&totals)),
                    stat.family.token(),
                )
            })
            .collect()
    };
    rsx! {
        div { class: "key-stats", role: "group", "aria-label": "Key stats",
            for (index, (label, value, colour)) in cells.into_iter().enumerate() {
                button {
                    key: "{index}",
                    class: "key-stats__cell",
                    r#type: "button",
                    "aria-label": "{label} {value}, open Stats",
                    onclick: move |_| {
                        sheet.set(None);
                        tab.set(crate::phone_build::Tab::Stats);
                        let _ = document::eval("window.scrollTo({ top: 0 });");
                    },
                    span { class: "key-stats__label", "{label}" }
                    span { class: "key-stats__value mono", style: "color: {colour}", "{value}" }
                }
            }
        }
    }
}

/// The foot of the Stats tab: one dropdown per position, every dashboard row offered,
/// grouped by section. A native select, so a phone opens its own picker.
#[component]
pub fn KeyStatChooser() -> Element {
    let choices = use_context::<KeyStatChoices>();
    let archetype = build_archetype();
    let rows = choices.rows(archetype.as_deref());
    rsx! {
        section { class: "mobile-sheet__panel key-stats-chooser",
            h3 { class: "popover-inline__title", "Top of the screen" }
            if archetype.is_none() {
                p { class: "key-stats-chooser__note", "Choose an archetype to change these." }
            }
            for (at, current) in rows.into_iter().enumerate() {
                label { key: "{at}", class: "key-stats-chooser__row",
                    span { class: "key-stats-chooser__place mono", "{at + 1}" }
                    select {
                        class: "key-stats-chooser__select",
                        disabled: archetype.is_none(),
                        "aria-label": "Key stat {at + 1}",
                        onchange: {
                            let archetype = archetype.clone();
                            move |evt: Event<FormData>| {
                                if let Some(archetype) = archetype.as_deref() {
                                    choices.set(archetype, at, &evt.value());
                                }
                            }
                        },
                        for section in StatSection::ALL {
                            optgroup { label: "{section.title()}",
                                for stat in stat_registry::in_section(section) {
                                    option {
                                        value: "{stat.id}",
                                        selected: stat.id == current.id,
                                        "{short_label(stat)}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{DatasetId, PowerDatabase};
    use std::path::PathBuf;

    /// Every archetype every server offers has a default four, each a real dashboard row — so
    /// a fork that adds an archetype, or a renamed row, fails here rather than on a phone.
    #[test]
    fn every_archetype_on_every_server_has_four_real_defaults() {
        for (_, four) in DEFAULTS {
            for id in four {
                assert!(stat_registry::by_id(id).is_some(), "{id} is not a stat row");
            }
        }
        for id in FALLBACK {
            assert!(stat_registry::by_id(id).is_some(), "{id} is not a stat row");
        }
        for dataset in DatasetId::ALL {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../contract/{}/bundle.json.gz",
                dataset.as_str()
            ));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
            let database = PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads");
            let archetypes = database.archetypes().expect("archetypes parse");
            assert!(
                !archetypes.all().is_empty(),
                "{} lists no archetypes",
                dataset.as_str()
            );
            for archetype in archetypes.all() {
                assert!(
                    DEFAULTS.iter().any(|(id, _)| *id == archetype.id),
                    "{} has no key-stat defaults ({})",
                    archetype.id,
                    dataset.as_str()
                );
            }
        }
    }

    #[test]
    fn defense_and_resistance_say_which_they_are() {
        let def = stat_registry::by_id("def_smashing").unwrap();
        let res = stat_registry::by_id("res_smashing").unwrap();
        assert_ne!(short_label(def), short_label(res));
        assert_eq!(
            short_label(stat_registry::by_id("recharge").unwrap()),
            "Recharge"
        );
    }
}
