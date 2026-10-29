//! Whether the Rule-of-5 alert is wanted, and whether it should skip mez resistance, kept across
//! reloads on the same `localStorage` document channel as [`crate::stats_store`] and
//! [`crate::picker_store`] (the beta's `useUIStore.ruleOf5AlertEnabled`).
//!
//! A preference, not build state: whether you want to be told about wasted bonuses is a property
//! of you, not of the character, so it survives switching builds and datasets exactly as the
//! stat config does. Missing or corrupt state keeps the alert ON — the default has to be the one
//! that reports, since the failure this alert exists to catch is a build quietly paying for
//! nothing.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

const KEY_ALERTS: &str = "sk-alerts";

#[derive(Serialize, Deserialize)]
#[serde(tag = "schema_version", content = "data")]
enum StoredAlerts {
    #[serde(rename = "1")]
    V1 { rule_of_five: bool },
    /// Adds the opt-out for mez-resistance bonuses, which builds stack past five routinely.
    #[serde(rename = "2")]
    V2 {
        rule_of_five: bool,
        ignore_mez_resistance: bool,
    },
}

/// The alert settings as one value, so restore and persist move them together.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AlertPrefs {
    pub rule_of_five: bool,
    pub ignore_mez_resistance: bool,
}

/// Persist the settings (fire-and-forget, like [`crate::picker_store::persist`]).
pub fn persist(prefs: AlertPrefs) {
    let stored = StoredAlerts::V2 {
        rule_of_five: prefs.rule_of_five,
        ignore_mez_resistance: prefs.ignore_mez_resistance,
    };
    let Ok(json) = serde_json::to_string(&stored) else {
        return;
    };
    let js = format!("try {{ localStorage.setItem({KEY_ALERTS:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

/// The persisted settings, or `None` (→ keep the defaults) for missing or corrupt state.
pub async fn load() -> Option<AlertPrefs> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_ALERTS:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    let text = value.as_str()?;
    Some(decode(serde_json::from_str(text).ok()?))
}

fn decode(stored: StoredAlerts) -> AlertPrefs {
    match stored {
        StoredAlerts::V1 { rule_of_five } => AlertPrefs {
            rule_of_five,
            ignore_mez_resistance: false,
        },
        StoredAlerts::V2 {
            rule_of_five,
            ignore_mez_resistance,
        } => AlertPrefs {
            rule_of_five,
            ignore_mez_resistance,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v1_record_keeps_its_setting_and_warns_about_mez() {
        let stored =
            serde_json::from_str(r#"{"schema_version":"1","data":{"rule_of_five":false}}"#)
                .unwrap();
        assert_eq!(
            decode(stored),
            AlertPrefs {
                rule_of_five: false,
                ignore_mez_resistance: false
            }
        );
    }

    #[test]
    fn v2_round_trips() {
        let prefs = AlertPrefs {
            rule_of_five: true,
            ignore_mez_resistance: true,
        };
        let json = serde_json::to_string(&StoredAlerts::V2 {
            rule_of_five: prefs.rule_of_five,
            ignore_mez_resistance: prefs.ignore_mez_resistance,
        })
        .unwrap();
        assert_eq!(decode(serde_json::from_str(&json).unwrap()), prefs);
    }
}
