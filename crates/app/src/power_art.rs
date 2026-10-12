//! The power and enhancement icons the PNG poster draws, as decoded pixels.
//!
//! The rest of the app shows a power's art by handing the browser a URL
//! ([`crate::view::icons::power_icon_url`]) and letting it fetch and decode. The poster cannot:
//! it composites into an [`RgbaImage`] in Rust, on both targets, with no DOM in reach — so it
//! needs the actual pixels, and it needs them before [`crate::export_image::render_to_png`] is
//! called, because that function is pure and synchronous and is worth keeping that way.
//!
//! So the bytes are gathered here, ahead of the render, one strategy per target — the same
//! split [`crate::data_source`] already makes for the dataset bundles, and for the same reason:
//!   desktop — the icons are embedded (`include_dir!`), so the app draws posters offline;
//!   web     — only the icons THIS build uses are fetched, from the asset tree the page is
//!             already serving.
//!
//! The web half is what the split is for. The full `powers/` tree is 7.3 MB across 2,991 files,
//! against a wasm bundle that is about 7 MB — embedding it uniformly would have roughly doubled
//! what every visitor downloads, to serve a modal most of them never open, and to hand the
//! renderer 2,967 icons no single build can use. A build holds at most a few dozen powers, so
//! the web half fetches a few dozen files, on the first poster and never again.
//!
//! Failure is per-icon and silent by design, which is the one place this module does NOT follow
//! Rule 1, and deliberately: a power whose art will not load draws as a tile without a picture,
//! exactly as every tile did before this module existed. The alternative is refusing to export
//! a build over one missing PNG. The poster is a picture of the build, not a reading of it — no
//! number on it comes from here, so a missing icon cannot make it say anything untrue.

use coh_data::CharacterState;
use image::RgbaImage;
use std::collections::HashMap;

/// Decoded power art, keyed by the icon filename the export states, lower-cased — the same key
/// [`crate::view::icons::power_icon_url`] builds its URL from, so the poster and the DOM agree
/// on which file is which power's.
#[derive(Clone, Default, PartialEq)]
pub struct PowerArt {
    icons: HashMap<String, RgbaImage>,
    /// Slotted enhancements' base icons and rarity frames, keyed by their path under
    /// `Enhancements/` (see [`enhancement_key`]).
    enhancements: HashMap<String, RgbaImage>,
}

impl PowerArt {
    /// The decoded art for a def's `icon` field, or `None` when the def carries none, the file
    /// was not gathered, or it failed to decode. The caller draws the tile either way.
    pub fn get(&self, icon: Option<&str>) -> Option<&RgbaImage> {
        let name = icon.map(str::trim).filter(|name| !name.is_empty())?;
        self.icons.get(&name.to_lowercase())
    }

    /// The decoded art behind an enhancement icon or frame URL — the URL
    /// [`crate::panels::powers::slot_icon_url`] or `slot_overlay_url` gave the DOM, so the poster
    /// draws the same picture the power card does. `None` when it was not gathered.
    pub fn enhancement(&self, url: &str) -> Option<&RgbaImage> {
        self.enhancements.get(enhancement_key(url)?)
    }
}

/// The part of an enhancement art URL below `Enhancements/` — `IO Sets/Bonesnap.png`. The same
/// on both targets, where the URL's own prefix is not: it is the bundled folder asset's address.
fn enhancement_key(url: &str) -> Option<&str> {
    url.rsplit_once("/Enhancements/").map(|(_, key)| key)
}

/// Every enhancement art URL the poster could draw for this build: each slotted piece's base
/// icon and the frame over it, deduplicated. A piece whose icon does not resolve contributes
/// nothing, as on the power card, which shows its text chip instead.
pub fn enhancement_urls(build: &CharacterState, database: &crate::shell::Db) -> Vec<String> {
    let origin = build.origin.as_deref();
    let mut urls: Vec<String> = build
        .all_selected()
        .flat_map(|power| power.slots.iter().flatten())
        .filter_map(|enh| {
            let icon = crate::panels::powers::slot_icon_url(enh, database)?;
            Some([
                icon,
                crate::panels::powers::slot_overlay_url(enh, database, origin),
            ])
        })
        .flatten()
        .collect();
    urls.sort();
    urls.dedup();
    urls
}

/// Every icon filename the poster could draw for this build — its picked powers and its
/// granted ones, since the poster draws both — deduplicated and lower-cased.
///
/// Read off each power's def (`Power.extra`'s `icon`), never derived from the power's name: the
/// filenames do not follow the display names, and guessing one would be the hardcode Rule 0
/// forbids. A power whose def will not resolve contributes nothing and draws a bare tile.
pub fn icon_names(build: &CharacterState, database: &crate::shell::Db) -> Vec<String> {
    let mut names: Vec<String> = build
        .all_selected()
        .filter_map(|power| {
            crate::view::power_view::resolve_power_def(
                database,
                &power.powerset,
                &power.internal_name,
            )
        })
        .filter_map(|def| def.extra.get("icon").and_then(|value| value.as_str()))
        .map(|icon| icon.trim().to_lowercase())
        .filter(|icon| !icon.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Gather and decode the art for one build. Never fails as a whole: an icon that cannot be
/// read is left out of the map (see the module note).
pub async fn load_for(build: &CharacterState, database: &crate::shell::Db) -> PowerArt {
    let mut icons = HashMap::new();
    for name in icon_names(build, database) {
        if let Some(bytes) = icon_bytes(&name).await {
            if let Ok(decoded) = image::load_from_memory(&bytes) {
                icons.insert(name, decoded.to_rgba8());
            }
        }
    }
    let mut enhancements = HashMap::new();
    for url in enhancement_urls(build, database) {
        let Some(key) = enhancement_key(&url) else {
            continue;
        };
        if let Some(bytes) = enhancement_bytes(&url, key).await {
            if let Ok(decoded) = image::load_from_memory(&bytes) {
                enhancements.insert(key.to_string(), decoded.to_rgba8());
            }
        }
    }
    PowerArt {
        icons,
        enhancements,
    }
}

/// Desktop: the vendored `Enhancements/` tree, embedded like the power art (2.2 MB, against the
/// power tree's 12).
#[cfg(not(target_arch = "wasm32"))]
async fn enhancement_bytes(_url: &str, key: &str) -> Option<Vec<u8>> {
    static ART: include_dir::Dir<'_> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets/img/Enhancements");
    ART.get_file(key).map(|file| file.contents().to_vec())
}

/// Web: the URL the power card's `<img>` already loaded, so it is usually in the cache.
#[cfg(target_arch = "wasm32")]
async fn enhancement_bytes(url: &str, _key: &str) -> Option<Vec<u8>> {
    let resp = gloo_net::http::Request::get(url).send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.binary().await.ok()
}

/// Desktop: the vendored `powers/` tree, embedded. The same files the webview is served, so a
/// poster drawn offline is the poster drawn online.
#[cfg(not(target_arch = "wasm32"))]
async fn icon_bytes(name: &str) -> Option<Vec<u8>> {
    static ICONS: include_dir::Dir<'_> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets/img/powers");
    ICONS.get_file(name).map(|file| file.contents().to_vec())
}

/// Web: fetched from the folder asset the page already serves, so an icon the user has seen in
/// a power row is already in the browser's cache by the time the poster asks for it.
#[cfg(target_arch = "wasm32")]
async fn icon_bytes(name: &str) -> Option<Vec<u8>> {
    let url = crate::view::icons::power_icon_url(Some(name));
    let resp = gloo_net::http::Request::get(&url).send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.binary().await.ok()
}
