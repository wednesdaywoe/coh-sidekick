//! A dropdown that shows an icon beside every choice — the closed button and each row of the
//! open list. A native `<select>` can only draw text, and the identity menus need the game's
//! archetype, origin and powerset art in them.
//!
//! The WAI-ARIA "select-only combobox": focus never leaves the button, the arrow keys move a
//! highlight through the list (`aria-activedescendant`), Enter picks, Escape closes, and typing
//! jumps to a matching name. The list is `position: fixed` and placed from the button's box, so
//! the header popover or a grid surface can't clip it; a surface holding an open list is lifted
//! over its neighbours in app.css. A transparent backdrop takes the outside click and the wheel,
//! so nothing scrolls out from under an open list.
//!
//! Never put one inside a `<label>`: a click anywhere in a label re-clicks its control, so
//! picking a row would reopen the list. Name it with `labelled_by` or `aria_label` instead.

use dioxus::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use web_time::{Duration, Instant};

/// One choice. `blocked` is the reason it can't be picked, shown in the row; `None` = pickable.
#[derive(Clone, PartialEq, Debug)]
pub struct IconOption {
    pub value: String,
    pub label: String,
    pub icon: Option<String>,
    pub blocked: Option<String>,
}

impl IconOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>, icon: Option<String>) -> Self {
        IconOption {
            value: value.into(),
            label: label.into(),
            icon,
            blocked: None,
        }
    }
}

/// Typed letters run together into one search while they come this close together.
const TYPEAHEAD_GAP: Duration = Duration::from_millis(600);
/// How far Page Up / Page Down move the highlight.
const PAGE: usize = 8;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// Place the open list under its button, or above it when there's more room there, and keep it
/// inside the window. Rects are window pixels and `left`/`top` sit inside the UI scale's `zoom`
/// on `<html>` ([`crate::ui_scale`]), hence the divide — the ⋮ menu's rule. On a phone the
/// bottom tab bar sits above the sheet a list opens in, so the list stops at the bar's top.
fn place_list_js(trigger_id: &str, list_id: &str) -> String {
    format!(
        "(function(){{\
var t=document.getElementById('{trigger_id}'),l=document.getElementById('{list_id}');\
if(!t||!l)return;\
var z=parseFloat(getComputedStyle(document.documentElement).zoom)||1,m=8,g=4;\
var r=t.getBoundingClientRect(),bot=innerHeight,n=document.querySelector('.mobile-nav');\
if(n){{var nr=n.getBoundingClientRect();if(nr.height>0&&nr.top<bot)bot=nr.top;}}\
l.style.minWidth=(r.width/z)+'px';l.style.maxHeight='none';\
var h=l.getBoundingClientRect().height;\
var below=bot-r.bottom-m-g,above=r.top-m-g;\
var up=h>below&&above>below,lh=Math.min(h,up?above:below);\
l.style.maxHeight=(lh/z)+'px';\
var w=l.getBoundingClientRect().width;\
var x=Math.max(m,Math.min(r.left,innerWidth-m-w)),y=up?r.top-g-lh:r.bottom+g;\
l.style.left=(x/z)+'px';l.style.top=(y/z)+'px';l.style.visibility='visible';\
var s=l.querySelector('.is-active')||l.querySelector('[aria-selected=true]');\
if(s)s.scrollIntoView({{block:'nearest'}});\
}})()"
    )
}

fn scroll_into_view_js(id: &str) -> String {
    format!("var e=document.getElementById('{id}');if(e)e.scrollIntoView({{block:'nearest'}});")
}

/// The nearest pickable row from `from` moving by `step` (±1), stopping at the ends. `from`
/// itself counts when `inclusive`. `None` when every row that way is blocked.
fn step_enabled(rows: &[IconOption], from: usize, step: isize, inclusive: bool) -> Option<usize> {
    let mut index = from as isize;
    if !inclusive {
        index += step;
    }
    while index >= 0 && (index as usize) < rows.len() {
        if rows[index as usize].blocked.is_none() {
            return Some(index as usize);
        }
        index += step;
    }
    None
}

/// The row a typed prefix lands on: the first pickable label starting with it, searching from
/// the row after `from` and wrapping, so a repeated single letter walks through its matches.
fn typeahead_match(rows: &[IconOption], from: usize, typed: &str) -> Option<usize> {
    let typed = typed.to_lowercase();
    let repeat = typed.chars().count() > 1 && typed.chars().all(|c| typed.starts_with(c));
    let needle: String = if repeat {
        typed.chars().take(1).collect()
    } else {
        typed.clone()
    };
    let start = if needle.chars().count() == 1 {
        from + 1
    } else {
        from
    };
    (0..rows.len())
        .map(|offset| (start + offset) % rows.len())
        .find(|&index| {
            rows[index].blocked.is_none() && rows[index].label.to_lowercase().starts_with(&needle)
        })
}

#[component]
pub fn IconSelect(
    /// The choices, in display order. The empty-value row (`placeholder`) is added in front.
    options: Vec<IconOption>,
    /// The chosen value; empty or unknown shows the placeholder.
    value: String,
    /// What the empty choice reads as, e.g. "— choose origin —".
    placeholder: String,
    onchange: EventHandler<String>,
    #[props(default)] disabled: bool,
    /// Extra classes on the button, so a caller frames it like the field it replaces.
    #[props(default)]
    class: String,
    #[props(default)] aria_label: Option<String>,
    /// The id of the visible label naming this control.
    #[props(default)]
    labelled_by: Option<String>,
) -> Element {
    let id = use_hook(|| NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let trigger_id = format!("icon-select-{id}");
    let list_id = format!("icon-select-{id}-list");
    let option_id = move |index: usize| format!("icon-select-{id}-opt-{index}");

    let mut open = use_signal(|| false);
    let mut active = use_signal(|| 0usize);
    let mut typed = use_signal(|| (String::new(), Instant::now()));
    let mut trigger = use_signal(|| None::<std::rc::Rc<MountedData>>);

    let mut rows = Vec::with_capacity(options.len() + 1);
    rows.push(IconOption::new("", placeholder.clone(), None));
    rows.extend(options.iter().cloned());
    let selected = rows.iter().position(|row| row.value == value).unwrap_or(0);
    let current = rows[selected].clone();

    let refocus = move || async move {
        if let Some(button) = trigger.peek().clone() {
            let _ = button.set_focus(true).await;
        }
    };
    let open_at = move |index: usize| {
        active.set(index);
        open.set(true);
    };
    let mut move_to = move |index: usize| {
        active.set(index);
        document::eval(&scroll_into_view_js(&option_id(index)));
    };
    let pick = {
        let rows = rows.clone();
        move |index: usize| {
            open.set(false);
            if let Some(row) = rows.get(index).filter(|row| row.blocked.is_none()) {
                if row.value != value {
                    onchange.call(row.value.clone());
                }
            }
            spawn(refocus());
        }
    };

    let onkeydown = {
        let rows = rows.clone();
        let mut pick = pick.clone();
        let mut open_at = open_at;
        move |evt: KeyboardEvent| {
            if disabled {
                return;
            }
            let key = evt.key();
            let last = rows.len() - 1;
            let is_open = open();
            let here = if is_open { active() } else { selected };
            // A printable key extends the search, unless it is a space that starts one.
            if let Key::Character(text) = &key {
                let (buffer, at) = typed();
                let running = at.elapsed() < TYPEAHEAD_GAP && !buffer.is_empty();
                if text != " " || running {
                    evt.prevent_default();
                    let search = if running {
                        format!("{buffer}{text}")
                    } else {
                        text.clone()
                    };
                    typed.set((search.clone(), Instant::now()));
                    let found = typeahead_match(&rows, here, &search);
                    match (is_open, found) {
                        (true, Some(index)) => move_to(index),
                        (false, found) => open_at(found.unwrap_or(selected)),
                        _ => {}
                    }
                    return;
                }
            }
            // Any other key ends a search, so the next letter starts a new one.
            typed.set((String::new(), Instant::now()));
            if !is_open {
                if matches!(key, Key::ArrowDown | Key::ArrowUp | Key::Enter)
                    || key == Key::Character(" ".into())
                {
                    evt.prevent_default();
                    open_at(selected);
                }
                return;
            }
            let target = match key {
                Key::ArrowDown => step_enabled(&rows, here, 1, false),
                Key::ArrowUp => step_enabled(&rows, here, -1, false),
                Key::Home => step_enabled(&rows, 0, 1, true),
                Key::End => step_enabled(&rows, last, -1, true),
                Key::PageDown => step_enabled(&rows, (here + PAGE).min(last), -1, true),
                Key::PageUp => step_enabled(&rows, here.saturating_sub(PAGE), 1, true),
                Key::Enter => {
                    evt.prevent_default();
                    pick(here);
                    return;
                }
                Key::Character(ref c) if c == " " => {
                    evt.prevent_default();
                    pick(here);
                    return;
                }
                // Kept from the popover around it, which closes on the same key.
                Key::Escape => {
                    evt.prevent_default();
                    evt.stop_propagation();
                    open.set(false);
                    return;
                }
                // Tab leaves without choosing, the way it leaves a native select.
                Key::Tab => {
                    open.set(false);
                    return;
                }
                _ => return,
            };
            evt.prevent_default();
            if let Some(index) = target {
                move_to(index);
            }
        }
    };

    let label_text = if current.blocked.is_some() {
        format!(
            "{} — {}",
            current.label,
            current.blocked.as_deref().unwrap_or_default()
        )
    } else {
        current.label.clone()
    };
    let is_open = open();
    let active_id = is_open.then(|| option_id(active()));

    rsx! {
        div { class: if is_open { "icon-select is-open" } else { "icon-select" },
            button {
                id: "{trigger_id}",
                class: "icon-select__trigger {class}",
                r#type: "button",
                role: "combobox",
                title: "{label_text}",
                disabled,
                "aria-haspopup": "listbox",
                "aria-expanded": is_open,
                "aria-controls": "{list_id}",
                "aria-activedescendant": active_id,
                "aria-label": aria_label.clone(),
                "aria-labelledby": labelled_by.clone(),
                onmounted: move |evt| trigger.set(Some(evt.data())),
                onclick: {
                    let mut open_at = open_at;
                    move |_| {
                        if open() {
                            open.set(false);
                        } else {
                            open_at(selected);
                            spawn(refocus());
                        }
                    }
                },
                onkeydown,
                if let Some(icon) = &current.icon {
                    img { class: "icon-select__icon", src: "{icon}", alt: "" }
                }
                span { class: "icon-select__label", "{current.label}" }
            }
            if is_open {
                div {
                    class: "icon-select__backdrop",
                    onclick: move |_| {
                        open.set(false);
                        spawn(refocus());
                    },
                }
                div {
                    id: "{list_id}",
                    class: "icon-select__list",
                    role: "listbox",
                    style: "visibility: hidden;",
                    // Placed once it exists, so it is measured at its real size.
                    onmounted: {
                        let script = place_list_js(&trigger_id, &list_id);
                        move |_| {
                            document::eval(&script);
                        }
                    },
                    "aria-label": aria_label.clone(),
                    "aria-labelledby": labelled_by.clone(),
                    for (index, row) in rows.iter().enumerate() {
                        div {
                            key: "{index}",
                            id: option_id(index),
                            role: "option",
                            class: if index == active() { "icon-select__option is-active" } else { "icon-select__option" },
                            "aria-selected": index == selected,
                            "aria-disabled": row.blocked.is_some(),
                            // Move, not enter: a list opening under a resting pointer must not
                            // take the highlight from the keyboard.
                            onmousemove: move |_| {
                                if active() != index {
                                    active.set(index);
                                }
                            },
                            onclick: {
                                let mut pick = pick.clone();
                                move |_| pick(index)
                            },
                            span { class: "icon-select__icon-slot",
                                if let Some(icon) = &row.icon {
                                    img { class: "icon-select__icon", src: "{icon}", alt: "" }
                                }
                            }
                            span { class: "icon-select__label", "{row.label}" }
                            if let Some(reason) = &row.blocked {
                                span { class: "icon-select__reason", "{reason}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(labels: &[(&str, bool)]) -> Vec<IconOption> {
        labels
            .iter()
            .map(|(label, blocked)| IconOption {
                value: label.to_lowercase(),
                label: label.to_string(),
                icon: None,
                blocked: blocked.then(|| "refused".to_string()),
            })
            .collect()
    }

    #[test]
    fn arrows_skip_refused_rows_and_stop_at_the_ends() {
        let list = rows(&[("A", false), ("B", true), ("C", false), ("D", true)]);
        assert_eq!(step_enabled(&list, 0, 1, false), Some(2));
        assert_eq!(step_enabled(&list, 2, 1, false), None);
        assert_eq!(step_enabled(&list, 3, -1, true), Some(2));
    }

    #[test]
    fn typing_walks_matches_and_skips_refused_ones() {
        let list = rows(&[
            ("Blaster", false),
            ("Brute", true),
            ("Bane", false),
            ("Controller", false),
        ]);
        assert_eq!(typeahead_match(&list, 0, "b"), Some(2));
        assert_eq!(typeahead_match(&list, 2, "bb"), Some(0));
        assert_eq!(typeahead_match(&list, 0, "co"), Some(3));
        assert_eq!(typeahead_match(&list, 0, "x"), None);
    }
}
