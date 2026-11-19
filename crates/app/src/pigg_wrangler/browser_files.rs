//! Getting at the player's game files from the browser, and reading only what is needed of them.
//!
//! Folder picking is JavaScript because both browser routes hand back JS objects: Chrome and
//! Edge's `showDirectoryPicker`, and `<input webkitdirectory>` everywhere else. Both return the
//! same shape, `{root, found: [[path, File]]}`, with only the `.pigg` files in `found`.

use coh_pigg::{parse_index, PiggEntry, PiggError};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = r#"
const LIMIT = 200000;
const TOO_MANY = 'That folder holds over 200,000 files, too many to search. Choose the folder '
  + 'City of Heroes is installed in, or one inside it.';
const isPigg = (name) => name.toLowerCase().endsWith('.pigg');

export function hasDirectoryPicker() {
  return typeof window.showDirectoryPicker === 'function';
}

async function walk(dir, path, found, seen) {
  for await (const entry of dir.values()) {
    if (++seen.count > LIMIT) throw new Error(TOO_MANY);
    const here = path + '/' + entry.name;
    if (entry.kind === 'directory') await walk(entry, here, found, seen);
    else if (isPigg(entry.name)) found.push([here, await entry.getFile()]);
  }
}

// null when the player closes the picker without choosing.
export async function pickDirectory() {
  let root;
  try {
    root = await window.showDirectoryPicker({ id: 'coh-game-folder' });
  } catch (e) {
    if (e && e.name === 'AbortError') return null;
    throw e;
  }
  const found = [];
  await walk(root, root.name, found, { count: 0 });
  return { root: root.name, found };
}

// null when nothing was chosen. Clears the input so choosing the same folder again still fires.
export function piggsFromInput(id) {
  const input = document.getElementById(id);
  if (!input) throw new Error('the folder input is missing from the page');
  const files = Array.from(input.files || []);
  input.value = '';
  if (files.length === 0) return null;
  if (files.length > LIMIT) throw new Error(TOO_MANY);
  const root = (files[0].webkitRelativePath || '').split('/')[0];
  const found = files
    .filter((file) => isPigg(file.name))
    .map((file) => [file.webkitRelativePath || file.name, file]);
  return { root, found };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasDirectoryPicker)]
    fn has_directory_picker_js() -> bool;
    #[wasm_bindgen(js_name = pickDirectory, catch)]
    async fn pick_directory_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = piggsFromInput, catch)]
    fn piggs_from_input_js(id: &str) -> Result<JsValue, JsValue>;
}

/// What a folder pick found: the chosen folder's name, and every `.pigg` under it.
pub struct Picked {
    pub root: String,
    pub found: Vec<(String, web_sys::File)>,
}

/// Whether this browser has Chrome's folder picker. Without it the page uses a folder input.
pub fn has_directory_picker() -> bool {
    has_directory_picker_js()
}

/// Opens Chrome's folder picker and walks the chosen folder. `None` if the player cancelled.
pub async fn pick_directory() -> Result<Option<Picked>, String> {
    picked(pick_directory_js().await.map_err(js_error)?)
}

/// What the folder input with this id holds. `None` if nothing was chosen.
pub fn piggs_from_input(id: &str) -> Result<Option<Picked>, String> {
    picked(piggs_from_input_js(id).map_err(js_error)?)
}

fn picked(value: JsValue) -> Result<Option<Picked>, String> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let field = |name: &str| js_sys::Reflect::get(&value, &name.into()).map_err(js_error);
    let root = field("root")?.as_string().unwrap_or_default();
    let mut found = Vec::new();
    for pair in js_sys::Array::from(&field("found")?).iter() {
        let pair = js_sys::Array::from(&pair);
        let path = pair
            .get(0)
            .as_string()
            .ok_or("a found file came back without a path")?;
        let file = pair
            .get(1)
            .dyn_into::<web_sys::File>()
            .map_err(|_| format!("{path}: came back as something other than a file"))?;
        found.push((path, file));
    }
    Ok(Some(Picked { root, found }))
}

/// Big enough for most bundles' whole table of contents in one read.
const FIRST_READ: f64 = 64.0 * 1024.0;

/// A bundle's table of contents, read from the front of the file only (sound.pigg is 137 MB).
pub async fn read_index(file: &web_sys::File) -> Result<Vec<PiggEntry>, String> {
    let size = file.size();
    let mut want = FIRST_READ.min(size);
    loop {
        let prefix = read_range(file, 0.0, want).await?;
        match parse_index(&prefix) {
            Err(PiggError::NeedMoreBytes { needed }) if needed as f64 > want && want < size => {
                want = (needed as f64).min(size);
            }
            other => return other.map_err(|error| error.to_string()),
        }
    }
}

async fn read_range(file: &web_sys::File, start: f64, end: f64) -> Result<Vec<u8>, String> {
    let slice = file.slice_with_f64_and_f64(start, end).map_err(js_error)?;
    let buffer = JsFuture::from(slice.array_buffer())
        .await
        .map_err(js_error)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// A thrown JS value as a sentence: an `Error`'s message, or whatever the value prints as.
fn js_error(value: JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}
