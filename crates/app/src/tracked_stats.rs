//! Tracked stats (the beta's `useUIStore.trackedStats`): the dashboard rows the user is chasing
//! with set bonuses. Clicking a stat row tracks it; the picker then marks every set with a bonus
//! that would move it, and every bonus ladder marks the lines that would.
//!
//! Keyed by [`StatDef::id`] rather than the beta's camelCase breakdown key. A bonus reaches a row
//! the same way the set-bonus finder files it — [`set_bonuses::stats_named`] then
//! [`stat_registry::rows_for_bonus_stat`] — so "does this bonus feed that row" has one answer in
//! the app, and the beta's hand-kept `TRACKED_STAT_TO_BONUS_STATS` map has no counterpart to
//! drift. A row no set bonus can feed (mez protection, debuff resistance) tracks fine and matches
//! nothing, which is the truth about it.
//!
//! A preference, not build state: it survives switching builds, and New build clears it, as the
//! beta's `resetForNewBuild` did.

use crate::panels::stat_registry::{self, StatDef};
use coh_math::set_bonuses::{self, NamedStats};
use dioxus::prelude::*;

const KEY_TRACKED_STATS: &str = "sk-tracked-stats";

/// The tracked rows, by [`StatDef::id`], in the order they were tracked.
#[derive(Clone, Copy, PartialEq)]
pub struct TrackedStats(pub Signal<Vec<String>>);

impl TrackedStats {
    pub fn contains(&self, id: &str) -> bool {
        self.0.read().iter().any(|tracked| tracked == id)
    }

    pub fn toggle(mut self, id: &str) {
        let mut tracked = self.0.write();
        match tracked.iter().position(|t| t == id) {
            Some(at) => {
                tracked.remove(at);
            }
            None => tracked.push(id.to_string()),
        }
    }

    pub fn clear(mut self) {
        if !self.0.peek().is_empty() {
            self.0.write().clear();
        }
    }
}

/// Provide [`TrackedStats`] at the shell root, restore it, persist it, and auto-track any
/// dashboard row whose total the Rule of 5 just started shrinking. Call after [`BuildTotals`] and
/// [`DashboardConfig`] are provided.
///
/// The beta auto-tracked a capped stat on EVERY recompute, so untracking one that was still
/// capped undid itself on the next edit. Here a row is auto-tracked once, when it becomes capped;
/// untracking it after that sticks.
///
/// [`BuildTotals`]: crate::panels::stats::BuildTotals
/// [`DashboardConfig`]: crate::panels::dashboards::DashboardConfig
pub fn use_tracked_stats_provider() -> TrackedStats {
    let tracked = use_signal(Vec::<String>::new);
    let handle = use_context_provider(|| TrackedStats(tracked));
    let mut restored = use_signal(|| false);
    use_future(move || async move {
        let mut tracked = tracked;
        if let Some(saved) = load().await {
            tracked.set(saved);
        }
        restored.set(true);
    });
    use_effect(move || {
        let tracked = tracked.read();
        if !*restored.read() {
            return;
        }
        persist(&tracked);
    });

    let totals = use_context::<crate::panels::stats::BuildTotals>().0;
    let dashboards = use_context::<crate::panels::dashboards::DashboardConfig>().0;
    let mut was_capped = use_signal(Vec::<&'static str>::new);
    use_effect(move || {
        // Restore first, or a capped row tracked here would be overwritten by the saved list.
        if !*restored.read() {
            return;
        }
        let capped: Vec<&'static str> = {
            let totals = totals.read();
            let dashboards = dashboards.read();
            dashboards
                .ids()
                .flat_map(|panel| dashboards.stats_in(panel).iter().copied())
                .filter_map(stat_registry::by_id)
                .filter(|stat| stat.resolve(&totals).rule_of_five_capped)
                .map(|stat| stat.id)
                .collect()
        };
        let newly: Vec<&'static str> = {
            let before = was_capped.peek();
            capped
                .iter()
                .copied()
                .filter(|id| !before.contains(id))
                .collect()
        };
        if !newly.is_empty() {
            let mut list = tracked;
            let mut list = list.write();
            for id in newly {
                if !list.iter().any(|t| t == id) {
                    list.push(id.to_string());
                }
            }
        }
        was_capped.set(capped);
    });
    handle
}

/// The dashboard rows one set-bonus effect would move, by id. Empty for a PvP-only effect (the
/// beta never highlighted those) and for an effect the vocabulary cannot place — the finder lists
/// those as faults; here they simply match nothing.
pub fn rows_for_effect(effect: &coh_data::SetBonusEffect) -> Vec<&'static str> {
    if effect.pvp {
        return Vec::new();
    }
    let NamedStats::Stats(stats) = set_bonuses::stats_named(effect) else {
        return Vec::new();
    };
    let mut ids: Vec<&'static str> = stats
        .into_iter()
        .flat_map(stat_registry::rows_for_bonus_stat)
        .map(|row| row.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// One set bonus that would move a tracked row: the picker's per-set chip.
#[derive(Clone, PartialEq, Debug)]
pub struct TrackedBonus {
    pub pieces: u8,
    pub stat: &'static StatDef,
    /// The bonus effect itself, for the chip's tooltip and its Rule-of-5 lookup.
    pub effect: coh_data::SetBonusEffect,
}

/// Every bonus in `set` that feeds a tracked row, one per (row, tier) — so a "+3% Smashing/Lethal
/// Defense" tier tracked on both halves shows twice (once per row) but a tier that names one row
/// twice shows once. Ordered by tier, then by the order the rows were tracked.
pub fn set_tracked_bonuses(set: &coh_data::IoSet, tracked: &[String]) -> Vec<TrackedBonus> {
    let mut found = Vec::new();
    for tier in &set.bonuses {
        for (rank, id) in tracked.iter().enumerate() {
            let Some(effect) = tier
                .effects
                .iter()
                .find(|effect| rows_for_effect(effect).contains(&id.as_str()))
            else {
                continue;
            };
            let Some(stat) = stat_registry::by_id(id) else {
                continue;
            };
            found.push((
                tier.pieces,
                rank,
                TrackedBonus {
                    pieces: tier.pieces,
                    stat,
                    effect: effect.clone(),
                },
            ));
        }
    }
    found.sort_by_key(|(pieces, rank, _)| (*pieces, *rank));
    found.into_iter().map(|(_, _, bonus)| bonus).collect()
}

fn persist(tracked: &[String]) {
    let Ok(json) = serde_json::to_string(tracked) else {
        return;
    };
    let js =
        format!("try {{ localStorage.setItem({KEY_TRACKED_STATS:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

/// The stored list, or `None` for missing or unreadable state. An id this build no longer has is
/// dropped rather than costing the rest of the list.
fn parse(text: &str) -> Option<Vec<String>> {
    let ids: Vec<String> = serde_json::from_str(text).ok()?;
    Some(
        ids.into_iter()
            .filter(|id| stat_registry::by_id(id).is_some())
            .collect(),
    )
}

async fn load() -> Option<Vec<String>> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_TRACKED_STATS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    parse(value.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::{IoSetCatalog, PowerDatabase};
    use std::path::PathBuf;

    fn catalog(dataset: &str) -> IoSetCatalog {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../contract/{dataset}/bundle.json.gz"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        PowerDatabase::from_gz_bytes(&bytes)
            .expect("bundle loads")
            .io_sets
            .expect("catalog present")
    }

    fn effect(stat: &str, pvp: bool) -> coh_data::SetBonusEffect {
        serde_json::from_value(serde_json::json!({ "stat": stat, "value": 3.0, "pvp": pvp }))
            .expect("effect parses")
    }

    fn sets_matching(catalog: &IoSetCatalog, id: &str) -> usize {
        let tracked = vec![id.to_string()];
        catalog
            .sets
            .values()
            .filter(|set| !set_tracked_bonuses(set, &tracked).is_empty())
            .count()
    }

    #[test]
    fn effect_reaches_its_row_and_its_pair() {
        assert_eq!(
            rows_for_effect(&effect("recharge", false)),
            vec!["recharge"]
        );
        // A typed defense bonus names its pair, as the game's "Smashing/Lethal" tiers do.
        let rows = rows_for_effect(&effect("defense_(smashing)", false));
        assert!(
            rows.contains(&"def_smashing") && rows.contains(&"def_lethal"),
            "{rows:?}"
        );
    }

    #[test]
    fn pvp_and_unknown_effects_match_nothing() {
        assert!(rows_for_effect(&effect("recharge", true)).is_empty());
        assert!(rows_for_effect(&effect("not_a_stat", false)).is_empty());
    }

    /// The beta's regression (2026-08-05): Max HP matched none of the sets that grant it. These
    /// floors are the beta test's, per dataset.
    #[test]
    fn real_catalogs_match_the_stats_people_chase() {
        for dataset in ["homecoming", "rebirth", "thunderspy"] {
            let catalog = catalog(dataset);
            for id in [
                "recharge",
                "health",
                "maxend",
                "runspeed",
                "endreduction",
                "res_fire",
            ] {
                assert!(
                    sets_matching(&catalog, id) > 0,
                    "{dataset}: {id} matches no set"
                );
            }
            assert!(sets_matching(&catalog, "health") > 50, "{dataset}: Max HP");
            assert!(
                sets_matching(&catalog, "mezres_hold") > 100,
                "{dataset}: hold resistance"
            );
            for id in ["absorb", "prot_hold", "stealth_pve"] {
                assert_eq!(sets_matching(&catalog, id), 0, "{dataset}: {id}");
            }
        }
    }

    #[test]
    fn bonuses_come_in_tier_order_one_per_row_and_tier() {
        let catalog = catalog("homecoming");
        let tracked = vec!["def_lethal".to_string(), "def_smashing".to_string()];
        for set in catalog.sets.values() {
            let bonuses = set_tracked_bonuses(set, &tracked);
            let keys: Vec<_> = bonuses.iter().map(|b| (b.pieces, b.stat.id)).collect();
            let mut deduped = keys.clone();
            deduped.dedup();
            assert_eq!(keys, deduped, "{}: a row repeats within a tier", set.name);
            assert!(
                keys.windows(2).all(|w| w[0].0 <= w[1].0),
                "{}: tier order",
                set.name
            );
        }
    }

    #[test]
    fn stored_list_drops_retired_ids() {
        assert_eq!(
            parse(r#"["recharge","no_such_row"]"#),
            Some(vec!["recharge".to_string()])
        );
        assert_eq!(parse("not json"), None);
    }
}
