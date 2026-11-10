//! Auction prices — the eighth edge function, and the estimated influence a build costs.
//!
//! `auction-prices` takes a list of Homecoming auction identifiers and answers a price per
//! identifier, cached server-side for an hour. The function itself is a proxy: it holds
//! `HC_AUCTION_API_KEY` and forwards to `hcvault.cityofheroes.dev/api/v1/auction/history/…`,
//! so a price here is a summary of that item's recent sales and nothing this planner computes.
//!
//! # Where it renders
//!
//! [`crate::enhancement_list`] prices every row it can and totals them under the header. It
//! shipped dark from 2026-09-17 until Homecoming's Vault went live on 2026-11-06, and the beta
//! never lit its own column either (`VITE_SHOW_AUCTION_PRICES` was set in no environment), so
//! this is a first light rather than a port.
//!
//! Only a Homecoming build is priced ([`prices_dataset`]). The proxy reads one market, and
//! pricing a Rebirth or Thunderspy build from it would answer a question about a server the
//! user is not playing on.
//!
//! # The identifier is the export's, not ours to spell
//!
//! An auction identifier is `5 Boosts.<record>.<record> <suffix>`, where `<record>` is the boost
//! name the game client prints for the piece. The beta ASSEMBLES that record from parts —
//! `<prefix>_<setId>_<letter>`, with `prefix` picked by whether the set id starts with
//! `superior_` (`auctionPrices.ts:37-62`). That reconstruction is wrong for 15 of Homecoming's
//! 227 sets, measured against the live auction API on 2026-09-17:
//!
//!   * the 10 purples spell attuned as `Superior_Attuned_Armageddon_A`, though their set id
//!     carries no `superior_` for the beta's rule to see;
//!   * the 5 superior winter sets spell it `Superior_Attuned_Avalanche_A` — the set id's own
//!     `superior_` DROPPED — while the superior ATOs keep theirs and double it
//!     (`Superior_Attuned_Superior_Blasters_Wrath_A`).
//!
//! Those two rules contradict each other, which is the tell that no rule was ever there to find.
//! The export already carries the answer: [`BoostIndex`] is keyed by the printed record name, and
//! [`BoostIndex::io_set_records`] runs the direction this needs. So the identifier is LOOKED UP,
//! and the 15 sets stop being special — they were only ever special to a string-assembler.
//!
//! That is also the Rule 0 reading. A `match` on a set name inside an encoder would be the bug
//! the rule is about; a foreign system's key format is fine to encode, and this encodes none of
//! it beyond the wrapper, because the part that varies comes from the export.
//!
//! # An unpriced row is not an error
//!
//! [`identifier_for`] answers `None` for every kind but an IO set piece, because those are all
//! the auction house sells under a `5 Boosts.` key — commons, origins and specials are bought
//! elsewhere or not at all. `not_found` on the wire is the same class of ordinary: the piece
//! exists and has no recent sales. Four sets were in that state on the day this landed, spelled
//! correctly and unsold across all six pieces.
//!
//! Rule 1 is owed to a call that FAILED, and that is [`CloudError`] — not to a piece the auction
//! house does not sell, which would light an error marker on most of a build.
//!
//! # The cap is refused, not absorbed
//!
//! The function does `[...new Set(identifiers)].slice(0, 100)` (`auction-prices/index.ts:121`):
//! over the cap it does not error, it answers a SHORT map, and a missing key is indistinguishable
//! from not-found. So [`fetch_prices`] chunks at the cap rather than trusting a caller to stay
//! under it. The cap is above any real build's distinct-piece count, which makes this headroom
//! rather than a live defect — and exactly the kind of headroom that stops being headroom quietly.

use super::{Cloud, CloudError};
use coh_data::boost_index::{BoostEntry, BoostIndex};
use coh_data::{DatasetId, Enhancement, EnhancementKind, Level};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The most identifiers one request may carry, from the function's own `MAX_IDENTIFIERS_PER_REQUEST`.
/// Mirrored rather than imported because it lives in a Deno module this build does not compile;
/// [`fetch_prices`] chunks on it so the mirror being stale costs a second round trip and never a
/// silently short answer.
const MAX_IDENTIFIERS_PER_REQUEST: usize = 100;

/// The crafting materials a shopping list prices alongside the pieces. These three are auction
/// items like any other, and their identifiers are fixed strings rather than anything derived —
/// salvage (type `11`) is not in the boost index, because it is not a boost.
///
/// The beta defines a fourth, `11 S_EnhancementConverter 0`, and never reads it. Not carried:
/// an unused constant is a claim that something needs it.
pub const CATALYST_IDENTIFIER: &str = "11 S_EnhancementCatalyst 0";
pub const BOOSTER_IDENTIFIER: &str = "11 S_EnhancementBooster 0";

/// One item's recent sales, as the function reports them.
///
/// Every price is `Option` because `not_found` rows carry nulls rather than being omitted, and
/// a zero would read as "free" on any surface that eventually renders this.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct PriceRow {
    pub raw_identifier: String,
    pub avg_price: Option<i64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub sample_count: Option<i64>,
    pub last_sale_at: Option<String>,
    pub fetched_at: String,
    /// The auction house knows the item and has no sales to report. Ordinary, not an error.
    #[serde(default)]
    pub not_found: bool,
}

impl PriceRow {
    /// The figure a cost estimate would use, or `None` when there is nothing to estimate from.
    /// `not_found` is folded in here so no caller has to remember that a found row can still
    /// carry a null average.
    pub fn estimate(&self) -> Option<i64> {
        if self.not_found {
            return None;
        }
        self.avg_price
    }
}

/// Whether the market behind `auction-prices` is the one a build on `dataset` buys from. The
/// proxy reads Homecoming's live auction house and nothing else; Brainstorm is a separate
/// server with its own market, so its builds are not priced from this one either.
pub fn prices_dataset(dataset: Option<DatasetId>) -> bool {
    dataset == Some(DatasetId::Homecoming)
}

/// An influence figure the way players say it: `850`, `450K`, `1.2M`, `3.4B`. One decimal
/// below ten of a unit and none above, because the figure is an average of recent sales and a
/// third significant digit would claim a precision it does not have.
pub fn format_influence(value: i64) -> String {
    let scaled = |unit: f64, suffix: &str| {
        let n = value as f64 / unit;
        if n < 10.0 {
            format!("{n:.1}{suffix}")
        } else {
            format!("{n:.0}{suffix}")
        }
    };
    // The thresholds sit half a unit early so a figure that would round up to `1000K` reads as
    // `1.0M` instead.
    match value {
        v if v >= 999_500_000 => scaled(1e9, "B"),
        v if v >= 999_500 => scaled(1e6, "M"),
        v if v >= 1_000 => format!("{}K", (value as f64 / 1e3).round() as i64),
        v => v.to_string(),
    }
}

#[derive(Serialize)]
struct PricesRequest<'a> {
    identifiers: &'a [String],
}

#[derive(Deserialize)]
struct PricesResponse {
    /// Absent keys and explicit nulls both mean "no row"; the wire uses null and the cap would
    /// produce absence, and neither is worth distinguishing at the call site.
    #[serde(default)]
    prices: HashMap<String, Option<PriceRow>>,
}

/// The auction identifier for a slotted piece, or `None` if the auction house does not sell it
/// under one.
///
/// `level` is the craft level to price at — the caller's, because [`Enhancement::level`] is
/// `None` for a piece that defers to the build's global IO level, and resolving that deferral
/// needs the build this function does not have.
///
/// Refuses rather than guesses when the index holds more than one record for the coordinate and
/// none matches the piece's attunement: that means the dataset disagrees with the build about
/// what this piece is, and a plausible pick would price the wrong item.
pub fn identifier_for(
    enhancement: &Enhancement,
    level: Option<Level>,
    index: &BoostIndex,
) -> Option<String> {
    let EnhancementKind::IoSet {
        set_id, piece_num, ..
    } = &enhancement.kind
    else {
        return None;
    };

    let record = index
        .io_set_records(set_id, *piece_num)
        .iter()
        .find(|name| match index.get(name) {
            Some(BoostEntry::IoSet { attuned, .. }) => *attuned == enhancement.attuned,
            _ => false,
        })?;

    Some(identifier_from_record(record, enhancement.attuned, level))
}

/// The wire form of a boost record. Split out so the wrapper is testable without an index, and
/// so the one place the format is spelled is not tangled with the lookup that feeds it.
///
/// The suffix is the piece's level minus one for a crafted piece and `0` for an attuned one,
/// which has no level to name. A crafted piece with no level resolved is priced at the suffix a
/// level-50 piece carries, because that is what the deferral means everywhere else in the
/// planner and inventing a second meaning here would price a different item.
fn identifier_from_record(record: &str, attuned: bool, level: Option<Level>) -> String {
    let suffix = if attuned {
        0
    } else {
        u32::from(level.map_or(50, Level::get)).saturating_sub(1)
    };
    format!("5 Boosts.{record}.{record} {suffix}")
}

/// Prices for a list of identifiers, as a map from identifier to its row.
///
/// An identifier the server had nothing for maps to `None`. Duplicates are collapsed before the
/// request and every input key is present in the answer, so a caller can index by the identifier
/// it built without checking twice.
pub async fn fetch_prices(
    cloud: &Cloud,
    identifiers: &[String],
) -> Result<HashMap<String, Option<PriceRow>>, CloudError> {
    let mut unique: Vec<String> = identifiers.to_vec();
    unique.sort();
    unique.dedup();

    let mut prices: HashMap<String, Option<PriceRow>> = HashMap::new();
    for chunk in unique.chunks(MAX_IDENTIFIERS_PER_REQUEST) {
        let response: PricesResponse = cloud
            .invoke("auction-prices", &PricesRequest { identifiers: chunk })
            .await?;
        for identifier in chunk {
            prices.insert(
                identifier.clone(),
                response.prices.get(identifier).cloned().flatten(),
            );
        }
    }
    Ok(prices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn influence_reads_the_way_players_say_it() {
        let cases = [
            (0, "0"),
            (850, "850"),
            (1_000, "1K"),
            (449_600, "450K"),
            (999_700, "1.0M"),
            (1_240_000, "1.2M"),
            (12_600_000, "13M"),
            (3_400_000_000, "3.4B"),
            (42_000_000_000, "42B"),
        ];
        for (value, expected) in cases {
            assert_eq!(format_influence(value), expected, "{value}");
        }
    }

    #[test]
    fn only_homecoming_is_priced() {
        assert!(prices_dataset(Some(DatasetId::Homecoming)));
        assert!(!prices_dataset(Some(DatasetId::Brainstorm)));
        assert!(!prices_dataset(Some(DatasetId::Rebirth)));
        assert!(!prices_dataset(None));
    }
}
