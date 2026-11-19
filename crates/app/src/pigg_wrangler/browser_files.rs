//! Getting at the player's game files from the browser, and reading only what is needed of them.
//!
//! Three routes in, all JavaScript because each hands back JS objects, and all returning the same
//! shape, `{root, found: [[path, File]]}`, with only the `.pigg` files in `found`:
//! - a folder dropped anywhere on the page, in every browser, with no dialog or prompt;
//! - Chrome and Edge's `showDirectoryPicker`;
//! - elsewhere, a plain multi-file input for the `.pigg` files themselves. Not a folder input:
//!   that one makes the browser say "Upload" and "upload 1,000 files to this site?", which
//!   put even the owner off (pigg-wrangler-web-11).

use coh_pigg::{extract_entry, parse_index, PiggEntry, PiggError};
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

// null when nothing was chosen. Clears the input so choosing the same files again still fires.
export function piggsFromInput(id) {
  const input = document.getElementById(id);
  if (!input) throw new Error('the file input is missing from the page');
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

// Dropped folders arrive as old-style entries, which work in every browser.
const fileOf = (entry) => new Promise((resolve, reject) => entry.file(resolve, reject));
const batchOf = (reader) => new Promise((resolve, reject) => reader.readEntries(resolve, reject));

async function walkEntry(entry, path, found, seen) {
  if (++seen.count > LIMIT) throw new Error(TOO_MANY);
  if (entry.isDirectory) {
    const reader = entry.createReader();
    // readEntries hands back up to 100 at a time; an empty batch is the end.
    for (let batch = await batchOf(reader); batch.length; batch = await batchOf(reader)) {
      for (const child of batch) await walkEntry(child, path + '/' + child.name, found, seen);
    }
  } else if (isPigg(entry.name)) {
    found.push([path, await fileOf(entry)]);
  }
}

// Drops wait here until the page asks for the next one, so Rust never runs inside a DOM listener.
const drops = [];
let waiting = null;
let installed = false;

function deliver(item) {
  if (waiting) {
    const resolve = waiting;
    waiting = null;
    resolve(item);
  } else {
    drops.push(item);
  }
}

function install(pageId) {
  if (installed) return;
  installed = true;
  let depth = 0;
  const mark = (on) => document.getElementById(pageId)?.classList.toggle('pigg-page--dragging', on);
  const carriesFiles = (e) => Array.from(e.dataTransfer?.types || []).includes('Files');
  window.addEventListener('dragenter', (e) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    if (depth++ === 0) mark(true);
  });
  window.addEventListener('dragleave', (e) => {
    if (!carriesFiles(e)) return;
    if (--depth <= 0) { depth = 0; mark(false); }
  });
  // Without this the browser opens a dropped file itself and leaves the page.
  window.addEventListener('dragover', (e) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  });
  window.addEventListener('drop', (e) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    depth = 0;
    mark(false);
    // Taken now: a drop's items are unreadable once this listener returns.
    const entries = Array.from(e.dataTransfer.items)
      .filter((item) => item.kind === 'file')
      .map((item) => item.webkitGetAsEntry())
      .filter(Boolean);
    (async () => {
      const found = [];
      const seen = { count: 0 };
      for (const entry of entries) await walkEntry(entry, entry.name, found, seen);
      const root = entries.length === 1 && entries[0].isDirectory ? entries[0].name : '';
      return { root, found };
    })().then((value) => deliver({ value }), (error) => deliver({ error }));
  });
}

// A browser address for bytes in memory, so an <img> can show a decoded picture.
export function objectUrl(bytes, mime) {
  return URL.createObjectURL(new Blob([bytes], { type: mime }));
}

export function revokeUrl(url) {
  URL.revokeObjectURL(url);
}

// The next folder or files dropped on the page: {value} or {error}. Never rejects.
export function nextDrop(pageId) {
  install(pageId);
  return new Promise((resolve) => {
    if (drops.length) resolve(drops.shift());
    else waiting = resolve;
  });
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasDirectoryPicker)]
    fn has_directory_picker_js() -> bool;
    #[wasm_bindgen(js_name = pickDirectory, catch)]
    async fn pick_directory_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = piggsFromInput, catch)]
    fn piggs_from_input_js(id: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = nextDrop)]
    async fn next_drop_js(page_id: &str) -> JsValue;
    #[wasm_bindgen(js_name = objectUrl)]
    fn object_url_js(bytes: &[u8], mime: &str) -> String;
    #[wasm_bindgen(js_name = revokeUrl)]
    fn revoke_url_js(url: &str);
}

/// A `blob:` address for bytes held in the page, handed back to the browser when dropped.
pub struct ObjectUrl(String);

impl ObjectUrl {
    pub fn new(bytes: &[u8], mime: &str) -> Self {
        ObjectUrl(object_url_js(bytes, mime))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for ObjectUrl {
    fn drop(&mut self) {
        revoke_url_js(&self.0);
    }
}

/// What a folder pick found: the chosen folder's name, and every `.pigg` under it.
pub struct Picked {
    pub root: String,
    pub found: Vec<(String, web_sys::File)>,
}

/// Whether this browser has Chrome's folder picker. Without it the page uses a file input.
pub fn has_directory_picker() -> bool {
    has_directory_picker_js()
}

/// Opens Chrome's folder picker and walks the chosen folder. `None` if the player cancelled.
pub async fn pick_directory() -> Result<Option<Picked>, String> {
    picked(pick_directory_js().await.map_err(js_error)?)
}

/// The `.pigg` files the file input with this id holds. `None` if nothing was chosen.
pub fn piggs_from_input(id: &str) -> Result<Option<Picked>, String> {
    picked(piggs_from_input_js(id).map_err(js_error)?)
}

/// Waits for the next folder or files dropped anywhere on the page with this id. The first call
/// starts listening; the page lives until a reload, so the listeners are never removed.
pub async fn next_drop(page_id: &str) -> Result<Option<Picked>, String> {
    let dropped = next_drop_js(page_id).await;
    let error = js_sys::Reflect::get(&dropped, &"error".into()).map_err(js_error)?;
    if !error.is_undefined() {
        return Err(js_error(error));
    }
    picked(js_sys::Reflect::get(&dropped, &"value".into()).map_err(js_error)?)
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

/// One file out of a bundle, read from its own stretch of the bundle and unpacked.
pub async fn read_entry(file: &web_sys::File, entry: &PiggEntry) -> Result<Vec<u8>, String> {
    let stored = read_stored(file, entry).await?;
    extract_entry(entry, &stored).map_err(|error| error.to_string())
}

/// One file's bytes as the bundle stores them, still packed: what a zip copies.
pub async fn read_stored(file: &web_sys::File, entry: &PiggEntry) -> Result<Vec<u8>, String> {
    let range = entry.stored_range();
    read_range(file, range.start as f64, range.end as f64).await
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
