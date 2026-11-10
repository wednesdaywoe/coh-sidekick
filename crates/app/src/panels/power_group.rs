//! A titled, collapsible column of power cards — the `.power-column` shape shared by every
//! picked-power section (Primary, Secondary, each pool, the epic pool), by each inherents
//! category, and by each band of the by-level ladder.
//!
//! Collapse lives per group rather than per panel: a build carries far more granted and pool
//! powers than a panel dragged small on the free grid can show at once, so folding the groups
//! a build isn't working on is what keeps the rest reachable without scrolling. The count stays
//! in the header while folded, because a group that hides both its cards and how many it has
//! reads as an empty one.
//!
//! The fold is deliberately not persisted — it is a "not right now", not a property of the
//! build. The one caller that persists it is the inherent panels
//! ([`crate::inherent_fold_store`]), which pass `fold` and own the state: there the fold is a
//! property of a panel the user placed, whose position, hidden set and pin all survive a
//! reload, and a fold that reset read as the app forgetting the panel.

use dioxus::prelude::*;

/// One collapsible group of cards.
///
/// Without `fold` the fold is a session-local signal seeded `false` — the "not right now"
/// reading, and the one every group inside the Powers panel and the by-level ladder wants.
///
/// A caller that wants the fold to outlive the session passes `fold`: a fold the caller owns
/// and persists, which replaces the local signal. The two are exclusive by shape — `fold` wins
/// when given — so a caller cannot hand both and have one silently ignored.
#[component]
pub fn PowerGroup(
    title: String,
    /// How many cards the group holds, shown beside the title so a folded group still says
    /// how much it is hiding.
    count: usize,
    /// A caller-owned fold, for a group whose open-or-folded state is a persisted preference
    /// (the inherent panels). Left `None`, the fold is the session-local signal above.
    #[props(default = None)]
    fold: Option<crate::inherent_fold_store::InherentFold>,
    children: Element,
) -> Element {
    let mut collapsed = use_signal(|| false);

    // One read for the whole render, whichever mode is in force: the caller-owned fold when
    // given, the session-local signal otherwise. Reading here (not inside the closures below)
    // is what subscribes this component to the state that answers, so a toggle from either
    // mode re-renders the group.
    let folded = fold
        .as_ref()
        .map_or_else(|| *collapsed.read(), |f| f.folded());

    rsx! {
        div { class: "power-column",
            button {
                class: "group-title",
                "aria-expanded": !folded,
                onclick: move |_| {
                    if let Some(fold) = &fold {
                        fold.toggle();
                    } else {
                        collapsed.toggle();
                    }
                },
                span {
                    class: if folded {
                        "group-title__caret caret--folded"
                    } else {
                        "group-title__caret"
                    },
                    "▼"
                }
                span { class: "group-title__text", "{title}" }
                span { class: "group-title__count", "{count}" }
            }
            if !folded {
                {children}
            }
        }
    }
}
