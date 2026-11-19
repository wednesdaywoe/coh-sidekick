//! Pigg Wrangler: browse and save the files inside the game's `.pigg` bundles, read from the
//! player's own install by the browser. Web only; the desktop app never routes here. A page of its
//! own at [`PATH`] rather than an overlay on the planner, so the planner's game data never loads.
//!
//! This file holds what both builds compile: the address, and the pure rules the page follows.
//! Everything that touches the browser is in the wasm-only modules below.

#[cfg(target_arch = "wasm32")]
mod browser_files;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
pub use page::Page;
#[cfg(any(target_arch = "wasm32", test))]
mod tree;

/// The page's address. The host answers any unknown path with `index.html` (wrangler.toml).
pub const PATH: &str = "/pigg-wrangler";

/// Whether the browser opened on this page. Read once, before the first render, so the planner
/// never starts loading underneath it.
#[cfg(target_arch = "wasm32")]
pub fn is_current_page() -> bool {
    web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .is_some_and(|pathname| is_page_path(&pathname))
}

// Typed and pasted by people, so a trailing slash or capitals still land here.
#[cfg(any(target_arch = "wasm32", test))]
fn is_page_path(pathname: &str) -> bool {
    pathname.trim_end_matches('/').eq_ignore_ascii_case(PATH)
}

/// The folder part of a found file's path: everything before the last `/`.
#[cfg(any(target_arch = "wasm32", test))]
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Splits found bundle paths into the folders that hold them, in path order, each with the
/// indices of its bundles. One install holds several sets (Homecoming's assets/live, beta,
/// experimental…), and mixing them would list the same file name many times over.
#[cfg(any(target_arch = "wasm32", test))]
fn group_by_folder(paths: &[String]) -> Vec<(String, Vec<usize>)> {
    let mut order: Vec<usize> = (0..paths.len()).collect();
    order.sort_by(|&a, &b| paths[a].to_lowercase().cmp(&paths[b].to_lowercase()));
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for index in order {
        let folder = folder_of(&paths[index]);
        match groups.last_mut() {
            Some((last, members)) if last == folder => members.push(index),
            _ => groups.push((folder.to_string(), vec![index])),
        }
    }
    groups
}

/// Which folder to open first: the one named `live` (Homecoming's main set), else the one with
/// the most bundles, the earliest on a tie.
#[cfg(any(target_arch = "wasm32", test))]
fn default_folder(groups: &[(String, Vec<usize>)]) -> usize {
    let name = |folder: &str| folder.rsplit('/').next().unwrap_or(folder).to_string();
    groups
        .iter()
        .position(|(folder, _)| name(folder).eq_ignore_ascii_case("live"))
        .or_else(|| {
            let most = groups.iter().map(|(_, members)| members.len()).max()?;
            groups.iter().position(|(_, members)| members.len() == most)
        })
        .unwrap_or(0)
}

/// `6696` → `6,696`.
#[cfg(any(target_arch = "wasm32", test))]
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A file size the way a file manager shows it: `137.0 MB`, `48 KB`, `512 bytes`.
#[cfg(any(target_arch = "wasm32", test))]
fn size_label(bytes: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_its_own_address_opens_the_page() {
        for path in ["/pigg-wrangler", "/pigg-wrangler/", "/Pigg-Wrangler"] {
            assert!(is_page_path(path), "{path}");
        }
        for path in ["/", "", "/builds/abc", "/pigg-wrangler/x", "/pigg"] {
            assert!(!is_page_path(path), "{path}");
        }
    }

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn an_install_root_opens_on_live() {
        let found = paths(&[
            "Homecoming/assets/live/bin.pigg",
            "Homecoming/assets/beta/bin.pigg",
            "Homecoming/assets/beta/sound.pigg",
            "Homecoming/assets/live/sound.pigg",
            "Homecoming/assets/launcher/a.pigg",
        ]);
        let groups = group_by_folder(&found);
        let folders: Vec<&str> = groups.iter().map(|(folder, _)| folder.as_str()).collect();
        assert_eq!(
            folders,
            [
                "Homecoming/assets/beta",
                "Homecoming/assets/launcher",
                "Homecoming/assets/live"
            ]
        );
        let live = &groups[default_folder(&groups)];
        assert_eq!(live.0, "Homecoming/assets/live");
        assert_eq!(live.1, [0, 3]);
    }

    #[test]
    fn without_live_the_biggest_folder_opens() {
        let groups = group_by_folder(&paths(&[
            "Sweet Tea/piggs/a.pigg",
            "Sweet Tea/tspy/a.pigg",
            "Sweet Tea/tspy/b.pigg",
        ]));
        assert_eq!(groups[default_folder(&groups)].0, "Sweet Tea/tspy");
    }

    #[test]
    fn numbers_read_the_way_people_write_them() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(6696), "6,696");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(size_label(137.0 * 1024.0 * 1024.0), "137.0 MB");
        assert_eq!(size_label(48.0 * 1024.0), "48 KB");
        assert_eq!(size_label(512.0), "512 bytes");
    }
}
