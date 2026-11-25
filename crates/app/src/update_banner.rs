//! The "Sidekick has received an update" banner (the beta's `UpdateBanner`), for a tab or
//! desktop window older than what is published.
//!
//! **Web**: any change to code or data changes the one name the page loads,
//! and comparing the live page's name with the running one is the whole check.
//! No version file, no build step, nothing to forget on deploy.
//! Re-asks every few minutes.
//!
//! **Desktop**: Newer live date → the same banner, displays the
//! releases page on startup, no refresh.

use dioxus::prelude::*;

/// How often an open tab asks. A release is rare and the request is one small page.
#[cfg(target_arch = "wasm32")]
const CHECK_EVERY_MS: u32 = 5 * 60 * 1000;

/// The deployed changelog — the file the web bundle is built from and the deploy publishes
/// beside it (see [`crate::app_info`]'s `include_str!` note). Its first date names the newest
/// build the site serves.
#[cfg(not(target_arch = "wasm32"))]
const CHANGELOG_URL: &str = "https://coh-sidekick.com/changelog.json";

/// How long a desktop build waits for that fetch before calling it not-news. Startup should
/// not visibly wait on a network at all; ten seconds is the far edge of patient.
#[cfg(not(target_arch = "wasm32"))]
const CHECK_TIMEOUT_SECS: u64 = 10;

/// Set just before the Refresh reload, so the new build opens What's new once.
const KEY_SHOW_WHATS_NEW: &str = "sk-show-whats-new";

/// The entry script's hashed name, read off a page's markup. Shared by both reads so the running
/// page and the live one are matched by the same pattern.
#[cfg(target_arch = "wasm32")]
const ENTRY_PATTERN: &str = r#"/assets\/([A-Za-z0-9_]+-dxh[0-9a-f]+\.js)/"#;

/// The build this tab is running, or `None` where the page names no hashed build.
#[cfg(target_arch = "wasm32")]
async fn running_build() -> Option<String> {
    let js = format!(
        "const s = document.querySelector('script[type=module][src*=\"/assets/\"]'); \
         const m = s && s.getAttribute('src').match({ENTRY_PATTERN}); \
         return m ? m[1] : null;"
    );
    document::eval(&js).await.ok()?.as_str().map(str::to_string)
}

/// Wait for the next check: the interval, or the moment the tab is looked at again, whichever
/// comes first. A tab brought back after a day should not show a stale build for five minutes.
#[cfg(target_arch = "wasm32")]
async fn wait_for_next_check() {
    let js = format!(
        "await new Promise(done => {{ \
           const wake = () => {{ if (document.visibilityState === 'visible') finish(); }}; \
           const finish = () => {{ clearTimeout(t); document.removeEventListener('visibilitychange', wake); done(); }}; \
           const t = setTimeout(finish, {CHECK_EVERY_MS}); \
           document.addEventListener('visibilitychange', wake); \
         }}); return true;"
    );
    let _ = document::eval(&js).await;
}

/// The build the server is serving now, or `None` when the page could not be fetched (offline,
/// a deploy mid-flight) — which is not news, so it shows nothing.
#[cfg(target_arch = "wasm32")]
async fn live_build() -> Option<String> {
    let js = format!(
        "try {{ \
           const r = await fetch('/', {{ cache: 'no-store' }}); \
           if (!r.ok) return null; \
           const m = (await r.text()).match({ENTRY_PATTERN}); \
           return m ? m[1] : null; \
         }} catch (_) {{ return null; }}"
    );
    document::eval(&js).await.ok()?.as_str().map(str::to_string)
}

/// Whether the last session's Refresh asked for What's new, clearing the request as it reads.
pub async fn take_whats_new_request() -> bool {
    let js = format!(
        "try {{ const v = localStorage.getItem({KEY_SHOW_WHATS_NEW:?}); \
           localStorage.removeItem({KEY_SHOW_WHATS_NEW:?}); return v === '1'; }} \
         catch (_) {{ return false; }}"
    );
    matches!(
        document::eval(&js).await.ok().and_then(|v| v.as_bool()),
        Some(true)
    )
}

/// A dated changelog group, read for its date and nothing else — the desktop check never
/// renders the live file, it only compares the stamp. A file whose groups parse but whose
/// items are junk still states a date, and a date is all news needs.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, allow(dead_code))]
#[derive(serde::Deserialize)]
struct DatedStamp {
    date: String,
}

/// `2026-10-20` as `(2026, 10, 20)`. Strict: three numeric segments and nothing else, so
/// HTML from the SPA fallback (which answers any path the deploy has no file for) can never
/// out-rank a real date by accident.
fn parse_date(date: &str) -> Option<(u32, u32, u32)> {
    let mut parts = date.split('-');
    let (year, month, day) = (
        parts.next()?.parse::<u32>().ok()?,
        parts.next()?.parse::<u32>().ok()?,
        parts.next()?.parse::<u32>().ok()?,
    );
    if parts.next().is_some() || !matches!(month, 1..=12) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

/// Is the live changelog's newest date past the one compiled into this build? Equal is
/// silence — a desktop build cut from the same commit as the deploy IS the latest release —
/// and a live date older than this build's own means this binary is ahead of the site (a dev
/// build), which is news to nobody.
fn live_date_is_news(live: &str, running: &str) -> bool {
    match (parse_date(live), parse_date(running)) {
        (Some(live), Some(running)) => live > running,
        _ => false,
    }
}

/// The newest date the deployed site's changelog carries, or `None` on every failure:
/// offline, a deploy mid-flight, a non-200, and — because the Worker answers unknown paths
/// with the SPA's index.html — a non-JSON body. All of them mean "no news", not "broken".
///
/// `dead_code` is allowed for test binaries: the fetch is a component edge — it fires once,
/// from the startup `use_future`, and no test runs a native component — while the unit tests
/// grade the pure comparison it feeds. The desktop QA check walks the wired path.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, allow(dead_code))]
async fn live_release_date() -> Option<String> {
    let body = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(CHECK_TIMEOUT_SECS))
        .build()
        .ok()?
        .get(CHANGELOG_URL)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    serde_json::from_str::<Vec<DatedStamp>>(&body)
        .ok()?
        .first()
        .map(|stamp| stamp.date.clone())
}

#[component]
pub fn UpdateBanner() -> Element {
    // The newer build found, held so a dismissal can name what it dismissed: a second release
    // after a dismissed one is news again.
    let mut found = use_signal(|| Option::<String>::None);
    let mut dismissed = use_signal(|| Option::<String>::None);

    use_future(move || async move {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(running) = running_build().await else {
                return;
            };
            loop {
                wait_for_next_check().await;
                if let Some(live) = live_build().await {
                    if live != running {
                        found.set(Some(live));
                    }
                }
            }
        }
        // Desktop asks once per app start — the owner's decision, in the module header —
        // and the ask is a date, not a build name: there is no live page to reload into,
        // only a release to go and download.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let running = crate::app_info::latest_changelog_date();
            if let (Some(live), Some(running)) = (live_release_date().await, running) {
                if live_date_is_news(&live, &running) {
                    found.set(Some(live));
                }
            }
        }
    });

    let Some(build) = found() else {
        return rsx! {};
    };
    if dismissed().as_deref() == Some(build.as_str()) {
        return rsx! {};
    }
    // One banner, two closes: a tab reloads into the new build, a desktop binary cannot
    // replace its own installer's work, so it hands the user to the releases page through
    // the same door the sign-in and the About links use.
    #[cfg(target_arch = "wasm32")]
    let action = rsx! {
        button {
            class: "update-banner__refresh",
            r#type: "button",
            onclick: move |_| {
                document::eval(&format!(
                    "try {{ localStorage.setItem({KEY_SHOW_WHATS_NEW:?}, '1'); }} catch (_) {{}} \
                     location.reload();"
                ));
            },
            "Refresh"
        }
        span { "to load it. What's new will open once it has." }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let action = rsx! {
        button {
            class: "update-banner__refresh",
            r#type: "button",
            onclick: move |_| {
                crate::app_actions::open_external(crate::app_actions::DESKTOP_RELEASES_URL);
            },
            "Download"
        }
        span { "the new desktop build — your saved builds and layouts open unchanged." }
    };
    rsx! {
        div { class: "update-banner", role: "status",
            span { "Sidekick has received an update! Please" }
            {action}
            button {
                class: "update-banner__dismiss",
                r#type: "button",
                "aria-label": "Dismiss",
                title: "Hide until the next update",
                onclick: move |_| dismissed.set(Some(build.clone())),
                "×"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The comparison the whole desktop check turns on, graded on the shapes the two reads
    /// actually produce — including the SPA's index.html, the one failure a desktop user
    /// hits when the deploy has no changelog file yet.
    #[test]
    fn a_newer_live_date_is_news_and_anything_else_is_silence() {
        assert!(live_date_is_news("2026-10-20", "2026-10-17"));
        assert!(live_date_is_news("2026-11-19", "2026-10-20"));
        assert!(live_date_is_news("2027-01-01", "2026-12-31"));
        assert!(
            !live_date_is_news("2026-10-20", "2026-10-20"),
            "same commit, same release"
        );
        assert!(
            !live_date_is_news("2026-10-17", "2026-10-20"),
            "a build ahead of the site (dev) says nothing"
        );
        for junk in [
            "<!DOCTYPE html>",
            "",
            "2026-10",
            "2026-10-20-extra",
            "2026-13-01",
            "2026-00-10",
            "2026-10-32",
        ] {
            assert!(
                !live_date_is_news(junk, "2026-10-17"),
                "{junk:?} must be silence"
            );
            assert!(
                !live_date_is_news("2026-10-20", junk),
                "own stamp {junk:?} must be silence"
            );
        }
    }

    /// The changelog compiled into this very test binary must state a date the check can
    /// read — if `latest_changelog_date` ever goes `None` (a moved file, a broken parse),
    /// the desktop banner silently dies, and this is the only place that death is visible.
    #[test]
    fn the_built_in_changelog_stamps_the_build() {
        let stamp =
            crate::app_info::latest_changelog_date().expect("the shipped changelog decodes");
        assert!(
            parse_date(&stamp).is_some(),
            "the changelog's newest date {stamp:?} must parse as YYYY-MM-DD"
        );
    }
}
