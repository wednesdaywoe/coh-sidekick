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
    const GB: f64 = MB * 1024.0;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} bytes")
    }
}

/// The kinds of file shown as text, as the Python tool's preview lists them.
#[cfg(any(target_arch = "wasm32", test))]
const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "cfg",
    "def",
    "mfx",
    "ms",
    "wtf",
    "inc",
    "powers",
    "powersets",
    "categories",
    "recipe",
    "contact",
    "storyarc",
    "taskset",
    "spawndef",
    "scrollset",
    "scriptdef",
    "npc",
    "villain",
    "zoneevent",
    "reward",
    "cape",
    "minimap",
    "bounds",
    "nd",
    "ctm",
    "anim",
];

/// Text past this many characters is cut, as the Python tool cuts it.
#[cfg(any(target_arch = "wasm32", test))]
const TEXT_SHOWN: usize = 100_000;

/// What a file inside a bundle is shown as.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Picture,
    Text,
    Other,
}

#[cfg(any(target_arch = "wasm32", test))]
fn kind_of(name: &str) -> Kind {
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    if extension.eq_ignore_ascii_case("texture") {
        Kind::Picture
    } else if TEXT_EXTENSIONS
        .iter()
        .any(|text| extension.eq_ignore_ascii_case(text))
    {
        Kind::Text
    } else {
        Kind::Other
    }
}

/// A file's bytes as text to read, bad bytes shown as �, and whether it was cut short.
///
/// Byte-order marks (U+FEFF, invisible) are dropped. Dioxus decodes each page update's strings with
/// a default `TextDecoder`, which swallows a leading one, and every string after it then lands one
/// character off: texturewords.ms, which starts with one, showed its name as "exturewords.ms".
#[cfg(any(target_arch = "wasm32", test))]
fn text_of(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes).replace('\u{feff}', "");
    match text.char_indices().nth(TEXT_SHOWN).map(|(cut, _)| cut) {
        Some(cut) => (text[..cut].to_string(), true),
        None => (text, false),
    }
}

/// `fire_blast.texture` saved as a picture: `fire_blast.png`.
#[cfg(any(target_arch = "wasm32", test))]
fn picture_name(file_name: &str, extension: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    format!("{stem}.{extension}")
}

/// A browser error as words a player can act on. The browser's own text ("Must be handling a user
/// gesture") means nothing to them; its kind says what happened.
#[cfg(any(target_arch = "wasm32", test))]
fn plain_reason(kind: &str, message: &str) -> String {
    match kind {
        "SecurityError" => "The browser wouldn't open its folder window this time. Press the \
                            button again, or drag the folder onto this page instead."
            .to_string(),
        "NotAllowedError" => "The browser wasn't allowed to read that folder. Press the button \
                              again and choose View files, or drag the folder onto this page."
            .to_string(),
        "NotFoundError" | "NotReadableError" => "A game file changed or moved after it was \
                                                 chosen, perhaps because the game's launcher \
                                                 is updating it. Choose the folder again."
            .to_string(),
        _ => message.to_string(),
    }
}

/// The most a zip made in the page may hold. The whole zip sits in the tab's memory, then the
/// browser copies it once more to save it; a gigabyte twice over is about what a tab can take.
/// The biggest archive in a Homecoming install (issue24's mapsMissions.pigg) stores 182 MB.
#[cfg(any(target_arch = "wasm32", test))]
const ZIP_LIMIT: u64 = 1 << 30;

/// Why a zip of `stored` bytes in `files` files can't be made, or `None` if it can.
#[cfg(any(target_arch = "wasm32", test))]
fn zip_refusal(files: usize, stored: u64) -> Option<String> {
    // coh_pigg::zip::MOST_FILES; that crate is in the web build only.
    if files > usize::from(u16::MAX) {
        return Some(format!(
            "That is {} files, and a zip can hold at most 65,535. Choose a smaller folder inside it.",
            thousands(files)
        ));
    }
    (stored > ZIP_LIMIT).then(|| {
        format!(
            "That comes to {}, more than the 1 GB a zip made in the browser can hold. Choose a \
             smaller folder inside it, or fewer files.",
            size_label(stored as f64)
        )
    })
}

/// What a folder's zip is called: the folder's own name, or the archive's for its top.
#[cfg(any(target_arch = "wasm32", test))]
fn zip_name(bundle_name: &str, folder: &str) -> String {
    let name = match folder.rsplit('/').next() {
        Some(last) if !last.is_empty() => last,
        _ => bundle_name.strip_suffix(".pigg").unwrap_or(bundle_name),
    };
    format!("{name}.zip")
}

/// The names inside one zip: each file under its path in its archive, as the Python tool does.
/// Two archives can hold the same path; the second copy goes under its archive's name instead of
/// overwriting the first when unzipped.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
struct ZipNames(std::collections::HashSet<String>);

#[cfg(any(target_arch = "wasm32", test))]
impl ZipNames {
    fn name(&mut self, bundle_name: &str, path: &str) -> String {
        if self.0.insert(path.to_string()) {
            return path.to_string();
        }
        let stem = bundle_name.strip_suffix(".pigg").unwrap_or(bundle_name);
        let mut name = format!("{stem}/{path}");
        let mut again = 2;
        while !self.0.insert(name.clone()) {
            name = format!("{stem} {again}/{path}");
            again += 1;
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_errors_read_as_what_to_do() {
        let gesture = plain_reason(
            "SecurityError",
            "Must be handling a user gesture to show a file picker.",
        );
        assert!(
            gesture.contains("drag the folder onto this page"),
            "{gesture}"
        );
        assert!(plain_reason("NotReadableError", "x").contains("Choose the folder again"));
        // The page's own errors are already sentences, and pass through.
        assert_eq!(
            plain_reason("Error", "That folder holds over 200,000 files."),
            "That folder holds over 200,000 files."
        );
    }

    #[test]
    fn zips_are_named_and_sized_before_they_are_made() {
        assert_eq!(
            zip_name("texture_gui.pigg", "texture_library/gui/icons"),
            "icons.zip"
        );
        assert_eq!(zip_name("texture_gui.pigg", ""), "texture_gui.zip");
        let mut names = ZipNames::default();
        assert_eq!(names.name("bin.pigg", "bin/powers.bin"), "bin/powers.bin");
        assert_eq!(
            names.name("bin_powers.pigg", "bin/powers.bin"),
            "bin_powers/bin/powers.bin"
        );
        assert_eq!(
            names.name("bin_powers.pigg", "bin/powers.bin"),
            "bin_powers 2/bin/powers.bin"
        );
        assert_eq!(zip_refusal(1_216, 49 << 20), None);
        assert_eq!(zip_refusal(1, ZIP_LIMIT), None);
        let big = zip_refusal(1, ZIP_LIMIT + 1).unwrap();
        assert!(
            big.starts_with("That comes to 1.0 GB, more than the 1 GB"),
            "{big}"
        );
        assert!(zip_refusal(70_000, 1).unwrap().contains("70,000 files"));
    }

    #[test]
    fn files_show_as_pictures_text_or_nothing() {
        assert_eq!(kind_of("Fire_Blast.TEXTURE"), Kind::Picture);
        assert_eq!(kind_of("Blaster.powersets"), Kind::Text);
        assert_eq!(kind_of("powers.bin"), Kind::Other);
        assert_eq!(kind_of("README"), Kind::Other);
        assert_eq!(picture_name("fire_blast.texture", "png"), "fire_blast.png");
        let long = "é".repeat(TEXT_SHOWN + 1);
        let (text, cut) = text_of(long.as_bytes());
        assert!(cut && text.chars().count() == TEXT_SHOWN);
        assert_eq!(text_of(b"ok\xff"), ("ok\u{fffd}".to_string(), false));
        // A byte-order mark is dropped: Dioxus's decoder would swallow it and shift the page's text.
        assert_eq!(text_of("\u{feff}a\u{feff}b".as_bytes()).0, "ab");
    }

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
        assert_eq!(size_label(1.5 * 1024.0 * 1024.0 * 1024.0), "1.5 GB");
    }
}
