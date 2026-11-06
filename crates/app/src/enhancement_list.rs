//! The enhancement list — the build as a shopping list.
//!
//! A finished build is a plan to buy things. This is that plan: every slotted piece, grouped
//! with its count, plus the two consumables the slotting implies — a Catalyst for each piece
//! that has to be attuned, and an Enhancement Booster for each level a piece is boosted by.
//! The beta's `EnhancementListModal`, with the auction-price column the beta built and never
//! switched on.
//!
//! # What makes two pieces one row
//!
//! A row is a PURCHASE, and two copies belong on it only when buying one is buying the other.
//! So the grouping key is the piece's identity plus every axis that changes what you buy: its
//! craft level, whether it is attuned, and how far it is boosted. The beta keys on set + piece
//! name alone, which puts a level-25 and a level-50 common IO on one line — two different items
//! on the auction house, counted as one thing to acquire.
//!
//! Splitting on those axes is also what makes the two consumable counts EXACT. Under the beta's
//! coarser key a group holds a mix of boosted and unboosted copies, so ticking one off can only
//! scale the group's boosters proportionally and round — a guess about which copy you bought.
//! Here a group's copies are identical by construction, so `remaining × per-copy` is the answer
//! and nothing is rounded.
//!
//! # The two consumables are read from the data, not from the flag
//!
//! **Catalysts.** `attuned` is true of two different things: a piece the user chose to attune,
//! and a piece from a set that is attuned by nature — [`IoSet::max_level`] `<= 1`, the ATO and
//! event sets. The second kind never consumed a Catalyst. Counting the flag (as the beta does)
//! bills the user for one per ATO piece in the build.
//!
//! **Boosters.** `boost` is a booster combine on an IO and a signed relative level on an SO or a
//! Hamidon ([`Enhancement::boost`]) — one field, two mechanics, told apart by the kind. Summing
//! it across kinds is how the beta's list reaches a build that needs −3 boosters.
//!
//! # Ticking off
//!
//! Clicking a row marks one copy acquired; clicking a finished row clears it. Buying a build
//! takes more than one sitting, so the ticks live on the build
//! ([`CharacterState::enhancement_acquired`]): each one is an undo step, the autosave keeps it,
//! and a saved `.skif` carries it. A share does not.
//!
//! A tick is keyed by the piece as the file states it ([`purchase_key`]), not by its display
//! name, because a build reopened from a file comes back with its display names empty.
//!
//! # Prices
//!
//! On a Homecoming build each row is priced from recent auction house sales
//! ([`crate::cloud::auction`]), and the header totals what is still to buy. The auction house
//! sells set pieces, catalysts and boosters under the keys the proxy reads, so those are what
//! is priced; commons, origins and specials are not, and the hint says so rather than leaving
//! a total that quietly means less than it looks.
//!
//! A set piece with no recent sales is not a zero. It is left out of the total and counted
//! beside it, because a total that silently skips the rare pieces is lowest exactly where the
//! build is most expensive.
//!
//! The request is keyed on the FULL list, not what is left of it, so ticking a row off reprices
//! nothing — it moves a row's share from one side of the `remaining / total` to the other.
//!
//! The clipboard carries what is on screen, remaining counts and all. A copy button that
//! silently exports the full list while the surface shows a half-finished one is a small lie of
//! exactly the kind this codebase keeps paying for.

use crate::build_session::BuildSession;
use crate::cloud::account::Account;
use crate::cloud::auction::{self, PriceRow};
use crate::floating::FloatingPanel;
use crate::naming;
use crate::shell::Db;
use coh_data::boost_index::BoostIndex;
use coh_data::{CharacterState, Enhancement, EnhancementKind, PowerDatabase};
use dioxus::prelude::*;
use std::collections::{BTreeMap, HashMap};

// ============================================================
// The list.
// ============================================================

/// Which run of the list a section sits in. The variant order IS the order on screen: set
/// pieces first, because they are what a build is planned around and what costs the most to
/// assemble, then the commons, then the origin families, then the vendor tiers.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum SectionRank {
    IoSet,
    Generic,
    Special,
    Origin,
}

/// One purchasable line: a piece, in one configuration, and how many of it the build wants.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    /// The piece's [`purchase_key`]: stable across rebuilds of the list, so a tick survives an
    /// edit to an unrelated power, and across a save and reopen.
    pub key: String,
    /// The piece's own name. The set name is the section heading, not part of this.
    pub label: String,
    /// What distinguishes this configuration from another of the same piece
    /// ([`naming::enhancement_qualifiers`]).
    pub qualifiers: Vec<String>,
    pub count: usize,
    /// Catalysts one copy needs: 1 for a piece the user attuned, 0 otherwise.
    pub catalysts_each: usize,
    /// Enhancement Boosters one copy needs.
    pub boosters_each: usize,
    /// This piece names an IO set the loaded dataset does not carry, so whether it is attuned
    /// by nature — and therefore whether it needs a Catalyst — cannot be answered. Surfaced
    /// rather than guessed either way (Rule 1).
    pub set_unresolved: bool,
    /// What the auction house calls this piece, or `None` when it is not priced: not a set
    /// piece, not a Homecoming build, or a piece the boost index cannot name
    /// ([`auction::identifier_for`]).
    pub auction_id: Option<String>,
}

impl Item {
    /// The row as one line of text, at whatever count the caller is showing.
    fn line(&self, count: usize) -> String {
        let mut line = format!("{count}x {}", self.label);
        if !self.qualifiers.is_empty() {
            line.push_str(&format!(" ({})", self.qualifiers.join(", ")));
        }
        if self.set_unresolved {
            line.push_str(" — set not in this dataset");
        }
        line
    }
}

/// One heading and the rows under it — an IO set, the commons, a special family, a vendor tier.
#[derive(Clone, PartialEq, Debug)]
pub struct Section {
    pub heading: String,
    pub items: Vec<Item>,
}

/// Everything the build wants, grouped.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShoppingList {
    pub sections: Vec<Section>,
    /// Every slotted piece, counted once — the figure the header's `remaining/total` is out of.
    pub total_pieces: usize,
    /// The build is on a dataset the auction house prices and the dataset can name its pieces.
    pub priced: bool,
}

/// The axes that make two copies the same purchase. Ordering is the on-screen order, so the
/// grouping map's own iteration is the sort.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct GroupKey {
    rank: SectionRank,
    heading: String,
    /// Where the piece sits in its set. Set pieces list in set order rather than alphabetically,
    /// because that is the order the picker and the set-bonus tooltip both show them in; every
    /// other kind leaves it 0 and sorts by name.
    piece_num: u8,
    label: String,
    /// `None` is a real state, not a missing level: an attuned piece has no craft level, and a
    /// piece with no stored level defers to the build's global IO level.
    level: Option<u8>,
    attuned: bool,
    boost: i8,
    /// Last, so it only ever splits a row the display fields would have merged, never reorders.
    purchase: String,
}

impl ShoppingList {
    /// Every enhancement in the build, grouped.
    ///
    /// Walks [`CharacterState::all_selected`] rather than naming the buckets, so a pick that
    /// lives somewhere the beta's five-bucket walk did not reach — a granted inherent — is on
    /// the list like any other.
    pub fn build(build: &CharacterState, database: &PowerDatabase) -> Self {
        let mut groups: std::collections::BTreeMap<GroupKey, Item> =
            std::collections::BTreeMap::new();
        let mut total_pieces = 0;
        let index = database
            .boost_index
            .as_ref()
            .filter(|_| auction::prices_dataset(database.dataset_id()));

        for power in build.all_selected() {
            for enhancement in power.slots.iter().flatten() {
                total_pieces += 1;
                let (key, item) = describe(enhancement, database, index);
                groups
                    .entry(key)
                    .and_modify(|existing| existing.count += 1)
                    .or_insert(item);
            }
        }

        let mut sections: Vec<Section> = Vec::new();
        for (key, item) in groups {
            match sections.last_mut() {
                Some(section) if section.heading == key.heading => section.items.push(item),
                _ => sections.push(Section {
                    heading: key.heading,
                    items: vec![item],
                }),
            }
        }

        ShoppingList {
            sections,
            total_pieces,
            priced: index.is_some(),
        }
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.sections
            .iter()
            .flat_map(|section| section.items.iter())
    }
}

/// One slotted piece as its group key and the row it opens.
fn describe(
    enhancement: &Enhancement,
    database: &PowerDatabase,
    index: Option<&BoostIndex>,
) -> (GroupKey, Item) {
    let label = naming::enhancement_name(enhancement).to_string();
    let qualifiers = naming::enhancement_qualifiers(enhancement);

    let (rank, heading, piece_num, set_unresolved, inherently_attuned) = match &enhancement.kind {
        EnhancementKind::IoSet {
            set_id,
            set_name,
            piece_num,
            ..
        } => {
            let set = database
                .io_sets
                .as_ref()
                .and_then(|catalog| catalog.get(set_id));
            let heading = if set_name.is_empty() {
                set_id.clone()
            } else {
                set_name.clone()
            };
            (
                SectionRank::IoSet,
                heading,
                *piece_num,
                set.is_none(),
                set.is_some_and(|set| set.attuned_only),
            )
        }
        EnhancementKind::GenericIo { .. } => (
            SectionRank::Generic,
            "Generic IOs".to_string(),
            0,
            false,
            false,
        ),
        EnhancementKind::Special { category, .. } => (
            SectionRank::Special,
            special_family_label(category, database),
            0,
            false,
            false,
        ),
        EnhancementKind::Origin { tier, .. } => (
            SectionRank::Origin,
            format!("{tier} Enhancements"),
            0,
            false,
            false,
        ),
    };

    // A booster is a thing you buy; a relative level is where you happen to be standing.
    let boosters_each = match &enhancement.kind {
        EnhancementKind::IoSet { .. } | EnhancementKind::GenericIo { .. } => {
            usize::try_from(enhancement.boost).unwrap_or(0)
        }
        EnhancementKind::Special { .. } | EnhancementKind::Origin { .. } => 0,
    };

    let catalysts_each = usize::from(enhancement.attuned && !inherently_attuned && !set_unresolved);

    let purchase = purchase_key(enhancement);
    let key = GroupKey {
        rank,
        heading,
        piece_num,
        label: label.clone(),
        level: enhancement.level.map(|level| level.get()),
        attuned: enhancement.attuned,
        boost: enhancement.boost,
        purchase: purchase.clone(),
    };
    let item = Item {
        key: purchase,
        label,
        qualifiers,
        count: 1,
        catalysts_each,
        boosters_each,
        set_unresolved,
        auction_id: index
            .and_then(|index| auction::identifier_for(enhancement, enhancement.level, index)),
    };
    (key, item)
}

/// One piece, in one configuration, spelled from the fields the `.skif` writes for it — so the
/// key a tick was saved under is the key the reopened build produces.
fn purchase_key(enhancement: &Enhancement) -> String {
    let piece = match &enhancement.kind {
        EnhancementKind::IoSet {
            set_id, piece_num, ..
        } => format!("set:{set_id}:{piece_num}"),
        EnhancementKind::GenericIo { stat, .. } => format!("generic:{stat}"),
        EnhancementKind::Special { .. } => format!("special:{}", enhancement.id),
        EnhancementKind::Origin { tier, stat, .. } => format!("origin:{tier}:{stat}"),
    };
    let level = enhancement
        .level
        .map(|level| level.get().to_string())
        .unwrap_or_default();
    format!(
        "{piece}|{level}|{}|{}",
        u8::from(enhancement.attuned),
        enhancement.boost
    )
}

/// A special family's display label, from the catalog's own family list — the same labels the
/// picker's tabs carry, so the list and the picker cannot name a family differently. The raw
/// tag is the fallback, for the reason every fallback here is the id.
fn special_family_label(category: &str, database: &PowerDatabase) -> String {
    database
        .enhancements
        .as_ref()
        .and_then(|catalog| {
            catalog
                .special_families()
                .into_iter()
                .find(|(_, tag, _)| *tag == category)
                .map(|(label, _, _)| label.to_string())
        })
        .unwrap_or_else(|| category.to_string())
}

// ============================================================
// What is left to buy.
// ============================================================

/// How many of each row the user has ticked off, keyed by [`Item::key`] — the build's own map.
type Acquired = BTreeMap<String, u32>;

/// The three running totals under the header, over the rows still outstanding.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Outstanding {
    pieces: usize,
    catalysts: usize,
    boosters: usize,
}

impl ShoppingList {
    /// What is still to be acquired. `acquired` empty is the whole list.
    fn outstanding(&self, acquired: &Acquired) -> Outstanding {
        let mut out = Outstanding::default();
        for item in self.items() {
            let remaining = item.count - acquired_count(acquired, item);
            out.pieces += remaining;
            out.catalysts += remaining * item.catalysts_each;
            out.boosters += remaining * item.boosters_each;
        }
        out
    }

    /// The list as text, at the counts on screen. Finished rows are dropped rather than struck
    /// through: strikethrough is a mark this destination does not have, and a line that says
    /// `0x` reads as an item you need none of rather than one you already own.
    fn to_text(&self, acquired: &Acquired, prices: Option<&Prices>) -> String {
        let mut lines: Vec<String> = Vec::new();
        for section in &self.sections {
            let live: Vec<(&Item, usize)> = section
                .items
                .iter()
                .map(|item| (item, item.count - acquired_count(acquired, item)))
                .filter(|(_, remaining)| *remaining > 0)
                .collect();
            if live.is_empty() {
                continue;
            }
            lines.push(section.heading.clone());
            for (item, remaining) in live {
                let mut line = format!("  {}", item.line(remaining));
                if let Some(RowPrice::Each(each)) = prices.and_then(|prices| item.price(prices)) {
                    let cost = auction::format_influence(each * remaining as i64);
                    line.push_str(&format!(" — ~{cost}"));
                }
                lines.push(line);
            }
            lines.push(String::new());
        }

        let out = self.outstanding(acquired);
        lines.push(format!("Catalysts needed: {}", out.catalysts));
        lines.push(format!("Enhancement Boosters needed: {}", out.boosters));
        if let Some(prices) = prices {
            let cost = self.cost(acquired, prices);
            let mut line = format!(
                "Estimated influence: ~{}",
                auction::format_influence(cost.remaining)
            );
            if cost.unpriced > 0 {
                line.push_str(&format!(" (plus {} with no recent sales)", cost.unpriced));
            }
            lines.push(line);
        }
        lines.join("\n")
    }
}

/// How many copies of a row are ticked off, never more than the row holds — an edit to the
/// build can shrink a row under its own tick count, and the stored count is kept for when it
/// grows back.
fn acquired_count(acquired: &Acquired, item: &Item) -> usize {
    acquired
        .get(&item.key)
        .map_or(0, |count| *count as usize)
        .min(item.count)
}

// ============================================================
// What it costs.
// ============================================================

/// Recent-sales rows by auction identifier, as [`auction::fetch_prices`] answers them.
type Prices = HashMap<String, Option<PriceRow>>;

/// A row's price, as the row draws it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum RowPrice {
    /// One copy's estimate, in influence.
    Each(i64),
    /// The auction house sells this piece and has no recent sales of it to go on.
    NoSales,
}

/// The influence still to spend and the whole build's worth, over everything that had a price.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Cost {
    remaining: i64,
    total: i64,
    /// Outstanding pieces, catalysts and boosters left out of `remaining` for want of sales.
    unpriced: usize,
}

fn unit_price(prices: &Prices, identifier: &str) -> Option<i64> {
    prices
        .get(identifier)
        .and_then(Option::as_ref)
        .and_then(PriceRow::estimate)
}

impl Item {
    /// `None` for a row the auction house does not price at all, which draws nothing.
    fn price(&self, prices: &Prices) -> Option<RowPrice> {
        let identifier = self.auction_id.as_deref()?;
        Some(unit_price(prices, identifier).map_or(RowPrice::NoSales, RowPrice::Each))
    }
}

impl ShoppingList {
    /// Everything to ask the auction house about: every priced piece, plus the two materials
    /// when the build needs any. Read off the full list so a tick never changes the request.
    fn auction_identifiers(&self) -> Vec<String> {
        if !self.priced {
            return Vec::new();
        }
        let full = self.outstanding(&Acquired::new());
        let mut identifiers: Vec<String> = self
            .items()
            .filter_map(|item| item.auction_id.clone())
            .collect();
        if full.catalysts > 0 {
            identifiers.push(auction::CATALYST_IDENTIFIER.to_string());
        }
        if full.boosters > 0 {
            identifiers.push(auction::BOOSTER_IDENTIFIER.to_string());
        }
        identifiers
    }

    fn cost(&self, acquired: &Acquired, prices: &Prices) -> Cost {
        let mut cost = Cost::default();
        for item in self.items() {
            let remaining = item.count - acquired_count(acquired, item);
            match item.price(prices) {
                Some(RowPrice::Each(each)) => {
                    cost.remaining += each * remaining as i64;
                    cost.total += each * item.count as i64;
                }
                Some(RowPrice::NoSales) => cost.unpriced += remaining,
                None => {}
            }
        }

        let out = self.outstanding(acquired);
        let full = self.outstanding(&Acquired::new());
        for (identifier, remaining, total) in [
            (auction::CATALYST_IDENTIFIER, out.catalysts, full.catalysts),
            (auction::BOOSTER_IDENTIFIER, out.boosters, full.boosters),
        ] {
            match unit_price(prices, identifier) {
                Some(each) => {
                    cost.remaining += each * remaining as i64;
                    cost.total += each * total as i64;
                }
                None => cost.unpriced += remaining,
            }
        }
        cost
    }
}

// ============================================================
// The modal.
// ============================================================

/// The list's open state, held at the shell root for the containment reason every modal here
/// shares: a `fixed` backdrop is contained by the grid's `transform`ed surfaces (see
/// [`crate::modal`]).
#[derive(Clone, Copy)]
pub struct EnhancementListOpen(pub Signal<bool>);

/// Mounted by the shell above both layout roots. Renders nothing while closed.
#[component]
pub fn EnhancementListHost(database: Option<Db>) -> Element {
    let mut open = use_context::<EnhancementListOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        FloatingPanel {
            id: "enhancement-list",
            title: "Enhancement list".to_string(),
            default_size: (620.0, 680.0),
            on_close: move |_| open.set(false),
            EnhancementListBody { database }
        }
    }
}

#[component]
fn EnhancementListBody(database: Option<Db>) -> Element {
    let session = use_context::<BuildSession>();
    let acquired = use_memo(move || session.build.read().enhancement_acquired.clone());
    let mut copied = use_signal(|| Option::<Result<(), String>>::None);

    // Every hook runs before either early return below. A `use_memo` reached only on some
    // renders shifts the hook index of everything after it, and BOTH of those branches flip
    // during a session: the dataset resolves under an already-open modal, and clearing the
    // build empties a list that was full a moment ago.
    let loading = database.is_none();
    // A memo, not a signal: the list is a pure function of the build and the dataset, and
    // derived state that is STORED is state that can be stale.
    let list = use_memo(move || match &database {
        Some(database) => ShoppingList::build(&session.build.read(), &database.0),
        None => ShoppingList::default(),
    });
    let out = use_memo(move || list.read().outstanding(&acquired.read()));
    // What the build wants with nothing ticked off — the denominator of all three tallies, and
    // a whole walk of the list, so it is memoized rather than recomputed per tally per render.
    let full = use_memo(move || list.read().outstanding(&Acquired::new()));

    // Prices. A memo between the list and the request so the request re-runs only when the
    // set of things to price changes, not on every tick or unrelated edit.
    let account = use_context::<Account>();
    let wanted = use_memo(move || list.read().auction_identifiers());
    let prices = use_resource(move || {
        let account = account.clone();
        let wanted = wanted();
        async move {
            if wanted.is_empty() {
                return Ok(None);
            }
            let cloud = account.prepared().await?;
            auction::fetch_prices(&cloud, &wanted).await.map(Some)
        }
    });
    let landed = move || -> Option<Prices> {
        prices
            .read()
            .as_ref()
            .and_then(|outcome| outcome.as_ref().ok())
            .cloned()
            .flatten()
    };
    let cost = use_memo(move || landed().map(|prices| list.read().cost(&acquired.read(), &prices)));

    if loading {
        return rsx! {
            div { class: "load-state", "Loading the dataset…" }
        };
    }
    if list.read().total_pieces == 0 {
        return rsx! {
            p { class: "enh-list__empty", "Nothing is slotted in this build yet." }
        };
    }
    // Read once for every row below rather than once per row.
    let shown_prices = landed();

    rsx! {
        div { class: "enh-list",
            div { class: "enh-list__totals",
                Tally {
                    label: "Pieces",
                    remaining: out().pieces,
                    total: list.read().total_pieces,
                }
                Tally {
                    label: "Catalysts",
                    remaining: out().catalysts,
                    total: full().catalysts,
                }
                Tally {
                    label: "Boosters",
                    remaining: out().boosters,
                    total: full().boosters,
                }
                if !wanted.read().is_empty() {
                    div {
                        class: "enh-list__tally",
                        title: "Estimated from recent Homecoming auction house sales",
                        span { class: "enh-list__tally-label", "Influence" }
                        match (&*prices.read(), cost()) {
                            (Some(Err(_)), _) => rsx! {
                                span { class: "enh-list__tally-total", "unavailable" }
                            },
                            (Some(Ok(_)), Some(cost)) => rsx! {
                                span { class: "enh-list__tally-value",
                                    "{auction::format_influence(cost.remaining)}"
                                }
                                if cost.remaining != cost.total {
                                    span { class: "enh-list__tally-total",
                                        "/ {auction::format_influence(cost.total)}"
                                    }
                                }
                            },
                            _ => rsx! {
                                span { class: "enh-list__tally-total", "pricing…" }
                            },
                        }
                    }
                }
                div { class: "enh-list__actions",
                    button {
                        class: "seg",
                        r#type: "button",
                        disabled: acquired.read().is_empty(),
                        onclick: move |_| {
                            session.commit(|build| build.enhancement_acquired.clear());
                            copied.set(None);
                        },
                        "Reset ticks"
                    }
                    button {
                        class: "seg is-primary",
                        r#type: "button",
                        onclick: move |_| async move {
                            let text = list.read().to_text(&acquired.read(), landed().as_ref());
                            copied.set(Some(crate::clipboard::copy(&text).await));
                        },
                        "Copy the list"
                    }
                }
            }

            p { class: "enh-list__hint",
                "Click a row to mark one off; click a finished row to put it back. Ticks are "
                "saved with the build."
            }

            if let Some(Err(error)) = &*prices.read() {
                p { class: "enh-list__status is-error",
                    "Auction prices could not be loaded: {error}."
                }
            }
            if let Some(cost) = cost() {
                p { class: "enh-list__hint",
                    "Influence is estimated from recent auction house sales, and covers set pieces, "
                    "catalysts and boosters."
                    if cost.unpriced == 1 {
                        " 1 item with no recent sales is not counted."
                    } else if cost.unpriced > 1 {
                        " {cost.unpriced} items with no recent sales are not counted."
                    }
                }
            }

            match copied() {
                Some(Ok(())) => rsx! {
                    p { class: "enh-list__status", "Copied — what you see is what was copied." }
                },
                Some(Err(reason)) => rsx! {
                    p { class: "enh-list__status is-error",
                        "Nothing was copied: {reason}."
                    }
                },
                None => rsx! {},
            }

            div { class: "enh-list__body",
                for section in list.read().sections.iter() {
                    div { key: "{section.heading}", class: "enh-list__section",
                        h3 { class: "enh-list__heading", "{section.heading}" }
                        ul { class: "enh-list__rows",
                            for item in section.items.iter() {
                                ItemRow {
                                    key: "{item.key}",
                                    item: item.clone(),
                                    acquired: acquired_count(&acquired.read(), item),
                                    price: shown_prices.as_ref().and_then(|prices| item.price(prices)),
                                    on_tick: move |key: String| {
                                        session.commit(move |build| {
                                            *build.enhancement_acquired.entry(key).or_insert(0) += 1;
                                        });
                                        copied.set(None);
                                    },
                                    on_clear: move |key: String| {
                                        session.commit(move |build| {
                                            build.enhancement_acquired.remove(&key);
                                        });
                                        copied.set(None);
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One `remaining / total` readout. The total is shown only once it differs from what is left,
/// so a fresh list reads as three plain numbers rather than three identical fractions.
#[component]
fn Tally(label: String, remaining: usize, total: usize) -> Element {
    rsx! {
        div { class: "enh-list__tally",
            span { class: "enh-list__tally-label", "{label}" }
            span { class: "enh-list__tally-value", "{remaining}" }
            if remaining != total {
                span { class: "enh-list__tally-total", "/ {total}" }
            }
        }
    }
}

/// One row. A `<button>` inside the `<li>` rather than a click handler on the row itself: this
/// is an action, and an action has to be reachable from the keyboard.
#[component]
fn ItemRow(
    item: Item,
    acquired: usize,
    price: Option<RowPrice>,
    on_tick: EventHandler<String>,
    on_clear: EventHandler<String>,
) -> Element {
    let remaining = item.count - acquired;
    let done = remaining == 0;
    let key = item.key.clone();

    rsx! {
        li { class: if done { "enh-list__row is-done" } else { "enh-list__row" },
            button {
                class: "enh-list__tick",
                r#type: "button",
                title: if done { "Put this row back on the list" } else { "Tick one off" },
                onclick: move |_| {
                    if done {
                        on_clear.call(key.clone());
                    } else {
                        on_tick.call(key.clone());
                    }
                },
                span { class: "enh-list__count",
                    if done { "✓" } else { "{remaining}×" }
                }
                span { class: "enh-list__label", "{item.label}" }
                if !item.qualifiers.is_empty() {
                    span { class: "enh-list__qualifiers", "{item.qualifiers.join(\", \")}" }
                }
                if acquired > 0 && !done {
                    span { class: "enh-list__of", "of {item.count}" }
                }
                // The consumables are told apart by their own word, not by a colour: a stat hue
                // here would say "this is a stat" about a count of things you go and buy.
                if item.catalysts_each > 0 && !done {
                    span { class: "enh-list__need",
                        "{remaining * item.catalysts_each} catalyst"
                    }
                }
                if item.boosters_each > 0 && !done {
                    span { class: "enh-list__need",
                        "{remaining * item.boosters_each} boosters"
                    }
                }
                if !done {
                    match price {
                        Some(RowPrice::Each(each)) => rsx! {
                            span {
                                class: "enh-list__price",
                                title: "About {auction::format_influence(each)} each",
                                "{auction::format_influence(each * remaining as i64)}"
                            }
                        },
                        Some(RowPrice::NoSales) => rsx! {
                            span { class: "enh-list__price is-unsold", "no recent sales" }
                        },
                        None => rsx! {},
                    }
                }
                if item.set_unresolved {
                    span { class: "enh-list__unresolved", "set not in this dataset" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coh_data::skif;
    use std::path::PathBuf;

    fn homecoming() -> PowerDatabase {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contract/homecoming/bundle.json.gz");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        PowerDatabase::from_gz_bytes(&bytes).expect("bundle loads")
    }

    /// One piece of every kind, as a file hands them over: display names blank.
    const FILE: &str = r#"{
      "version": 5,
      "build": {
        "dataset": "homecoming",
        "archetype": "tanker",
        "level": 50,
        "primary": {
          "id": "tanker/stone-armor",
          "powers": [{
            "internalName": "Granite_Armor",
            "level": 26,
            "slots": [
              { "type": "io-set", "setId": "absolute_amazement", "pieceNum": 1, "level": 50, "booster": 2 },
              { "type": "io-generic", "stat": "Defense", "level": 50 },
              { "type": "special", "category": "hamidon", "id": "endoplasm" },
              { "type": "origin", "stat": "Defense", "tier": "SO", "relativeLevel": 1 }
            ]
          }]
        },
        "secondary": { "id": "tanker/super-strength" }
      }
    }"#;

    fn keys(build: &CharacterState, database: &PowerDatabase) -> Vec<String> {
        ShoppingList::build(build, database)
            .items()
            .map(|item| item.key.clone())
            .collect()
    }

    /// A tick saved on a build picked in this session has to find its row once the build is
    /// reopened from a file, where every display name comes back blank.
    #[test]
    fn a_tick_finds_its_row_after_a_save_and_reopen() {
        let database = homecoming();
        let from_file = skif::decode(FILE, &database).expect("decodes").build;
        let mut picked = from_file.clone();
        for power in picked.primary.powers.iter_mut() {
            for piece in power.slots.iter_mut().flatten() {
                piece.name = format!("{} (picker name)", piece.id);
                if let EnhancementKind::IoSet { set_name, .. } = &mut piece.kind {
                    *set_name = "Absolute Amazement".to_string();
                }
            }
        }
        let before = keys(&picked, &database);
        assert_eq!(before.len(), 4, "one row per kind: {before:?}");

        let text = skif::encode(&picked, &database).expect("encodes");
        let reopened = skif::decode(&text, &database).expect("decodes").build;
        assert_eq!(keys(&reopened, &database), before);
    }

    fn row(estimate: Option<i64>) -> Option<PriceRow> {
        Some(PriceRow {
            raw_identifier: String::new(),
            avg_price: estimate,
            min_price: None,
            max_price: None,
            sample_count: None,
            last_sale_at: None,
            fetched_at: String::new(),
            not_found: estimate.is_none(),
        })
    }

    /// The set piece is priced under the key the export names it by, the two commons-and-origins
    /// rows are not priced at all, and the booster its +2 implies is asked for alongside it.
    #[test]
    fn a_homecoming_build_asks_for_its_set_pieces_and_materials() {
        let database = homecoming();
        let build = skif::decode(FILE, &database).expect("decodes").build;
        let list = ShoppingList::build(&build, &database);
        assert!(list.priced);

        let priced: Vec<&Item> = list.items().filter(|i| i.auction_id.is_some()).collect();
        assert_eq!(priced.len(), 1, "only the set piece: {priced:?}");
        let piece = priced[0].auction_id.clone().unwrap();
        assert!(
            piece.starts_with("5 Boosts.") && piece.ends_with(" 49"),
            "{piece}"
        );

        assert_eq!(
            list.auction_identifiers(),
            vec![piece, auction::BOOSTER_IDENTIFIER.to_string()]
        );
    }

    /// A tick moves a row's share out of what is left without repricing it, and a piece with no
    /// sales is counted beside the total instead of as zero inside it.
    #[test]
    fn the_cost_follows_the_ticks_and_names_what_it_left_out() {
        let database = homecoming();
        let build = skif::decode(FILE, &database).expect("decodes").build;
        let list = ShoppingList::build(&build, &database);
        let piece = list.items().find_map(|i| i.auction_id.clone()).unwrap();
        let key = list
            .items()
            .find(|i| i.auction_id.is_some())
            .unwrap()
            .key
            .clone();

        let mut prices: Prices = HashMap::new();
        prices.insert(piece.clone(), row(Some(1_000_000)));
        prices.insert(auction::BOOSTER_IDENTIFIER.to_string(), row(Some(100_000)));

        let fresh = list.cost(&Acquired::new(), &prices);
        assert_eq!(
            fresh,
            Cost {
                remaining: 1_200_000,
                total: 1_200_000,
                unpriced: 0
            }
        );

        let ticked: Acquired = [(key, 1)].into_iter().collect();
        let after = list.cost(&ticked, &prices);
        assert_eq!(
            after,
            Cost {
                remaining: 0,
                total: 1_200_000,
                unpriced: 0
            }
        );

        prices.insert(piece, row(None));
        let unsold = list.cost(&Acquired::new(), &prices);
        assert_eq!(
            unsold,
            Cost {
                remaining: 200_000,
                total: 200_000,
                unpriced: 1
            }
        );
    }
}
