//! The folder tree inside one bundle, and the file-name search across bundles. No browser here,
//! so both are tested natively; the page only draws them.

use std::collections::BTreeMap;

/// A folder inside a bundle: its subfolders, and the entries directly in it as indices into the
/// bundle's table of contents. Both sorted by name, ignoring case.
#[derive(Debug, Default)]
pub struct Dir {
    pub name: String,
    /// From the bundle's top, `""` for the top itself.
    pub path: String,
    pub dirs: Vec<Dir>,
    pub files: Vec<usize>,
    /// Every file at any depth below.
    pub count: usize,
}

#[derive(Default)]
struct Building {
    dirs: BTreeMap<String, Building>,
    files: Vec<(String, usize)>,
}

impl Dir {
    /// The tree of a bundle's entry paths, in table-of-contents order.
    pub fn build<'a>(paths: impl IntoIterator<Item = &'a str>) -> Dir {
        let mut root = Building::default();
        for (index, path) in paths.into_iter().enumerate() {
            let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            let Some(name) = parts.pop() else { continue };
            let mut here = &mut root;
            for part in parts {
                here = here.dirs.entry(part.to_string()).or_default();
            }
            here.files.push((name.to_lowercase(), index));
        }
        finish(String::new(), String::new(), root)
    }

    /// The folder at `path` below this one, if there is one.
    pub fn find(&self, path: &str) -> Option<&Dir> {
        path.split('/')
            .filter(|part| !part.is_empty())
            .try_fold(self, |dir, part| {
                dir.dirs.iter().find(|child| child.name == part)
            })
    }

    /// The tree's visible rows under this folder, opening only the folders `is_open` names.
    pub fn rows(&self, depth: usize, is_open: &dyn Fn(&str) -> bool, out: &mut Vec<Row>) {
        for dir in &self.dirs {
            let open = is_open(&dir.path);
            out.push(Row {
                depth,
                name: dir.name.clone(),
                path: dir.path.clone(),
                count: dir.count,
                has_dirs: !dir.dirs.is_empty(),
                open,
            });
            if open {
                dir.rows(depth + 1, is_open, out);
            }
        }
    }
}

fn finish(name: String, path: String, building: Building) -> Dir {
    let mut files = building.files;
    files.sort();
    let mut dirs: Vec<Dir> = building
        .dirs
        .into_iter()
        .map(|(name, child)| {
            let child_path = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}/{name}")
            };
            finish(name, child_path, child)
        })
        .collect();
    dirs.sort_by_cached_key(|dir| dir.name.to_lowercase());
    let count = files.len() + dirs.iter().map(|dir| dir.count).sum::<usize>();
    Dir {
        name,
        path,
        dirs,
        files: files.into_iter().map(|(_, index)| index).collect(),
        count,
    }
}

/// One line of the drawn tree.
pub struct Row {
    pub depth: usize,
    pub name: String,
    pub path: String,
    pub count: usize,
    pub has_dirs: bool,
    pub open: bool,
}

/// The words of a search, lowercased. Empty when there is nothing to search for.
pub fn search_words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether a lowercased path holds every word, anywhere and in any order, as the Python tool's
/// search does: `powers icons` finds `texture_library/gui/icons/powers/…`.
pub fn matches(words: &[String], lower_path: &str) -> bool {
    words.iter().all(|word| lower_path.contains(word.as_str()))
}

/// Every folder a path sits in, outermost first: `a/b/c.bin` → `a`, `a/b`.
pub fn folders_above(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut folder = String::new();
    let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    parts.pop();
    for part in parts {
        if !folder.is_empty() {
            folder.push('/');
        }
        folder.push_str(part);
        out.push(folder.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_count_everything_below_them() {
        let paths = [
            "bin/powers.bin",
            "texture_library/GUI/Icons/Powers/b.texture",
            "texture_library/GUI/Icons/Powers/A.texture",
            "texture_library/GUI/x.texture",
            "readme.txt",
        ];
        let root = Dir::build(paths);
        assert_eq!(root.count, 5);
        assert_eq!(root.files, [4]);
        let names: Vec<&str> = root.dirs.iter().map(|dir| dir.name.as_str()).collect();
        assert_eq!(names, ["bin", "texture_library"]);
        let gui = root.find("texture_library/GUI").unwrap();
        assert_eq!((gui.count, gui.files.as_slice()), (3, &[3][..]));
        let powers = root.find("texture_library/GUI/Icons/Powers").unwrap();
        assert_eq!(powers.path, "texture_library/GUI/Icons/Powers");
        // Sorted by name ignoring case: A before b, though b came first.
        assert_eq!(powers.files, [2, 1]);
        assert!(root.find("texture_library/gui").is_none());

        let open = |path: &str| path == "texture_library";
        let mut rows = Vec::new();
        root.rows(0, &open, &mut rows);
        let drawn: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.depth,
                    row.name.as_str(),
                    row.path.as_str(),
                    row.count,
                    row.has_dirs,
                    row.open,
                )
            })
            .collect();
        assert_eq!(
            drawn,
            [
                (0, "bin", "bin", 1, false, false),
                (0, "texture_library", "texture_library", 3, true, true),
                (1, "GUI", "texture_library/GUI", 3, true, false),
            ]
        );
        assert_eq!(
            folders_above(paths[2]),
            [
                "texture_library",
                "texture_library/GUI",
                "texture_library/GUI/Icons",
                "texture_library/GUI/Icons/Powers"
            ]
        );
    }

    #[test]
    fn every_search_word_must_appear_in_any_order() {
        let path = "texture_library/gui/icons/powers/fire_blast.texture";
        assert!(matches(&search_words("Powers  ICONS"), path));
        assert!(matches(&search_words("fire .texture"), path));
        assert!(!matches(&search_words("powers ice"), path));
        assert!(search_words("   ").is_empty());
    }
}
