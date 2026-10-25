//! The build browser's state — which tab is showing, the search, the filters, the pinned
//! author, whether the tag picker is open — kept across close/reopen and reloads on the same
//! `localStorage` document channel as [`crate::picker_store`] and [`crate::build_store`].
//!
//! The browser used to forget everything when its modal closed: the tab reset to the one the
//! menu row named, the search emptied, the filters back to default. That is the "resets to
//! default each time" a user filed against, and the answer is the one every other preference in
//! this app already takes — the state is the browser's, so it survives the browser the way the
//! picker's last location and the layout do.
//!
//! **What is saved is the whole surface, not just the query.** The tab is an option the user
//! selects as much as any filter, and "My builds" forgotten while "AFK farming" is remembered
//! is the same reset wearing a smaller hat. The pinned author is saved as a reduced row
//! ([`PinnedAuthor`]) rather than the RPC's [`crate::cloud::profile::AuthorSearchResult`]:
//! the chip draws the name and the handle, the query carries the id, and the similarity score
//! the RPC ranked on is "carried because it is in the row, not drawn" — there is nowhere for it
//! to go in a save, so the save does not carry it.
//!
//! Missing or corrupt state opens the browser the way a fresh install does: the menu row's
//! `initial` tab and the default filters. Corrupt means *rejected as a whole*, in
//! [`crate::picker_store`]'s Rule 1 — a stored `page` of zero is not a page to clamp, it is a
//! value this app never wrote, and a save that half-restores is a reset that only admits it on
//! the second look.

use crate::cloud::shared_builds::SearchFilters;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

const KEY_BROWSER: &str = "sk-browser-filters";

/// The tab tokens, as the persisted [`BrowserState`] spells them. Stored as wire strings rather
/// than the browser's own enum, for [`crate::picker_memory`]'s reason: a persisted tab outlives
/// a rename of the enum, and one spelling is used by the store, the validation and the tab
/// buttons, so a rename cannot desynchronise them.
pub mod view {
    pub const ALL: &str = "all";
    pub const FAVOURITES: &str = "favourites";
    pub const MINE: &str = "mine";
}

/// The author the browser's author filter is pinned to, reduced to what the pin draws and what
/// the query needs. `sim` is not here for the reason the module doc names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PinnedAuthor {
    pub user_id: String,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub build_count: i64,
}

/// The build browser's state, as one save.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowserState {
    /// Which tab is showing, as a [`view`] token.
    pub view: String,
    /// The search box's draft text, as typed. May differ from `filters.query`: a draft is
    /// committed on submit, and a typed-but-unsubmitted search is not a search the filters hold.
    pub query: String,
    /// The committed search, filters and page. `#[serde(default)]` so a state written before
    /// a field existed — or with one facet missing — restores the rest and reads the missing
    /// facet as unfiltered, the same answer a fresh install gets.
    #[serde(default)]
    pub filters: SearchFilters,
    /// The pinned author, or `None` for no pin. Defaulted for the same reason as `filters`.
    #[serde(default)]
    pub pinned: Option<PinnedAuthor>,
    /// Whether the tag picker is open. Defaulted for the same reason; a save without it closes.
    #[serde(default)]
    pub tags_open: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "schema_version", content = "data")]
enum StoredBrowser {
    #[serde(rename = "1")]
    V1(BrowserState),
}

/// Encode a state for storage, or `None` when it will not serialise — which for these fields
/// means nothing, but the write path is refused rather than written half.
pub fn encode(state: &BrowserState) -> Option<String> {
    let stored = StoredBrowser::V1(state.clone());
    serde_json::to_string(&stored).ok()
}

/// Decode a stored value, or `None` for missing-version, wrong-shape or failing-validation state.
///
/// The decode is kept apart from [`load`] for the reason [`crate::build_store`] keeps its codec
/// in `coh_data`: the decision of what a save is or is not is gradeable without a runtime, and
/// [`load`] is then the thin `eval` round trip every other load in this app is.
pub fn decode(text: &str) -> Option<BrowserState> {
    let StoredBrowser::V1(state) = serde_json::from_str(text).ok()?;
    validate(&state).then_some(state)
}

/// Whether a decoded state is one this app would have written.
///
/// Three checks, each one a value this writer cannot produce: a `view` token outside the three,
/// a `page` below one (the browser's page control is `saturating` and `.max(1)`), and a
/// `page_size` of zero (the browser never touches it, so the only zero is a foreign one).
/// Anything that fails is rejected whole — see the module doc for why a half-restore is the
/// reset this store exists to remove.
pub fn validate(state: &BrowserState) -> bool {
    matches!(
        state.view.as_str(),
        view::ALL | view::FAVOURITES | view::MINE
    ) && state.filters.page >= 1
        && state.filters.page_size >= 1
}

/// Persist the browser's state (fire-and-forget, like [`crate::picker_store::persist]).
/// Written by the one funnel [`crate::cloud::browser::BrowserModal`] routes every state
/// mutation through, so a change cannot reach the screen without reaching this line.
pub fn persist(state: &BrowserState) {
    let Some(json) = encode(state) else {
        return;
    };
    let js = format!("try {{ localStorage.setItem({KEY_BROWSER:?}, {json:?}); }} catch (_) {{}}");
    crate::storage::commit(js);
}

/// The persisted state if present and valid; `None` (→ the browser opens on its `initial`
/// tab with fresh filters) for missing, corrupt or invalid state.
pub async fn load() -> Option<BrowserState> {
    let js = format!(
        "try {{ return localStorage.getItem({KEY_BROWSER:?}); }} catch (_) {{ return null; }}"
    );
    let value = document::eval(&js).await.ok()?;
    let text = value.as_str()?;
    decode(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::shared_builds::SortBy;

    fn state() -> BrowserState {
        BrowserState {
            view: view::MINE.to_string(),
            query: "tanker psionic".to_string(),
            filters: SearchFilters {
                archetype: Some("tanker".to_string()),
                primary_set: Some("psionic-armor".to_string()),
                secondary_set: Some("dual-blades".to_string()),
                query: "tanker psionic".to_string(),
                author_id: Some("user-1".to_string()),
                author_name: None,
                tags: vec!["AFK farming".to_string(), "Budget".to_string()],
                sort_by: SortBy::Views,
                page: 3,
                page_size: 20,
            },
            pinned: Some(PinnedAuthor {
                user_id: "user-1".to_string(),
                handle: Some("handle".to_string()),
                display_name: "Display".to_string(),
                avatar_url: Some("https://example.org/a.png".to_string()),
                build_count: 7,
            }),
            tags_open: true,
        }
    }

    // The round trip is the contract: what the browser saved is what the next open reads,
    // pin and page and all.
    #[test]
    fn a_saved_state_round_trips_whole() {
        let state = state();
        let text = encode(&state).expect("encodes");
        let back = decode(&text).expect("decodes");
        assert_eq!(back, state);
    }

    // The tab token is the one field a typo silently turns into a different tab, so it is
    // the one validation is least willing to guess.
    #[test]
    fn an_unknown_view_token_is_not_a_saved_state() {
        let text = encode(&state()).expect("encodes");
        let foreign = text.replace(view::MINE, "everywhere");
        assert_eq!(decode(&foreign), None);
    }

    // A `page` of zero is not a page to clamp to one: it is a value this writer never wrote,
    // and Rule 1 rejects rather than recovers.
    #[test]
    fn a_zero_page_is_not_a_saved_state() {
        let text = encode(&state()).expect("encodes");
        let foreign = text.replace("\"page\":3", "\"page\":0");
        assert_eq!(decode(&foreign), None);
    }

    // The browser never writes a `page_size` of zero; one is a foreign value for the same
    // reason, and the search it would drive slices nothing.
    #[test]
    fn a_zero_page_size_is_not_a_saved_state() {
        let text = encode(&state()).expect("encodes");
        let foreign = text.replace("\"page_size\":20", "\"page_size\":0");
        assert_eq!(decode(&foreign), None);
    }

    // The facets default to unfiltered, so a state that lost one keeps the rest: a save
    // written before `tags` existed restores the archetype and the page, not nothing.
    #[test]
    fn missing_facets_restore_the_rest() {
        let text = r#"{
            "schema_version": "1",
            "data": {
                "view": "all",
                "query": "dps",
                "filters": {
                    "archetype": "blaster",
                    "query": "dps",
                    "page": 2,
                    "page_size": 20
                }
            }
        }"#;
        let back = decode(text).expect("decodes");
        assert_eq!(back.view, view::ALL);
        assert_eq!(back.filters.archetype.as_deref(), Some("blaster"));
        assert_eq!(back.filters.tags.len(), 0);
        assert_eq!(back.filters.page, 2);
        assert_eq!(back.filters.sort_by, SortBy::Newest);
        assert_eq!(back.pinned, None);
        assert!(!back.tags_open);
    }

    // A page a state does not name is page one — the page control's floor — and a page size
    // it does not name is the app's constant, never zero.
    #[test]
    fn a_missing_page_reads_as_page_one() {
        let text = r#"{
            "schema_version": "1",
            "data": {
                "view": "favourites",
                "query": "",
                "filters": {}
            }
        }"#;
        let back = decode(text).expect("decodes");
        assert_eq!(back.filters.page, 1);
        assert_eq!(back.filters.page_size, 20);
    }

    // The version tag is the contract with a future writer: a shape this decode does not know
    // is not a state with a problem, it is not a state, and the browser opens fresh rather
    // than half.
    #[test]
    fn an_unknown_schema_version_is_not_a_saved_state() {
        let text = r#"{"schema_version": "2", "data": {"view": "all"}}"#;
        assert_eq!(decode(text), None);
    }

    // Not JSON at all: the localStorage slot is this app's, but a foreign writer's bytes under
    // the key are still a foreign writer's, and they read as no save.
    #[test]
    fn non_json_is_not_a_saved_state() {
        assert_eq!(decode("not json at all"), None);
    }

    // The encode/decode boundary is the whole of what the write path can fail on; `persist`
    // itself is the `eval` round trip every other persist is, which a unit test cannot reach.
    #[test]
    fn validate_agrees_with_decode() {
        let state = state();
        assert!(validate(&state));
        assert!(decode(&encode(&state).expect("encodes")).is_some());
    }
}
