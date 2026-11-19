//! Pigg Wrangler: browse and save the files inside the game's `.pigg` bundles, read from the
//! player's own install by the browser. Web only; the desktop app never routes here. A page of its
//! own at [`PATH`] rather than an overlay on the planner, so the planner's game data never loads.

use dioxus::prelude::*;

/// The page's address. The host answers any unknown path with `index.html` (wrangler.toml).
pub const PATH: &str = "/pigg-wrangler";

/// Whether the browser opened on this page. Read once, before the first render, so the planner
/// never starts loading underneath it.
pub fn is_current_page() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.location().pathname().ok())
            .is_some_and(|pathname| is_page_path(&pathname))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

// Typed and pasted by people, so a trailing slash or capitals still land here.
#[cfg(any(target_arch = "wasm32", test))]
fn is_page_path(pathname: &str) -> bool {
    pathname.trim_end_matches('/').eq_ignore_ascii_case(PATH)
}

#[component]
pub fn Page() -> Element {
    rsx! {
        document::Title { "Pigg Wrangler · CoH Sidekick" }
        div { class: "pigg-page",
            header { class: "pigg-page__head",
                a { class: "pigg-page__back", href: "/", "← Back to the planner" }
                h1 { class: "pigg-page__name", "Pigg Wrangler" }
                p { class: "pigg-page__about",
                    "Browse data and save artwork and other assets from your CoH install."
                }
            }
            main { class: "pigg-page__body",
                // Shown on touch screens only (app.css): no phone or tablet can install the game.
                p { class: "pigg-page__touch",
                    "Pigg Wrangler reads the game's files from your computer, so it needs a \
                     Windows, Mac or Linux computer with City of Heroes installed."
                }
                // The folder picker goes here (pigg-wrangler-web-4).
                div { class: "pigg-page__workspace" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_page_path;

    #[test]
    fn only_its_own_address_opens_the_page() {
        for path in ["/pigg-wrangler", "/pigg-wrangler/", "/Pigg-Wrangler"] {
            assert!(is_page_path(path), "{path}");
        }
        for path in ["/", "", "/builds/abc", "/pigg-wrangler/x", "/pigg"] {
            assert!(!is_page_path(path), "{path}");
        }
    }
}
