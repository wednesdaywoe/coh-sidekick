//! Mobile bottom nav — the fixed <900px tab bar.
//!
//! Three tabs: Build, Stats, More (the phone-build-flow plan). Build is the planner itself; it
//! closes whatever is open, and when nothing is, scrolls back to the top. Stats and More open a
//! full-screen sheet that stops at the bar's top edge, so switching tabs is one tap. Stats holds
//! the dashboard's stat panels; More holds every menu and setting, plus the incarnate picker,
//! which only matters at 50 and so lost its own tab.
//!
//! Build Identity is a third sheet with no tab: the build bar at the top of the stack opens it
//! (see [`crate::build_bar`]), because the build's name belongs where it can be read, and a first
//! visit has to find where a build starts without opening More to look.
//!
//! The sheets hold no menus of their own. They list the header's popovers under
//! [`PopoverInline`], which makes each one draw its body in place under its title, so every
//! entry here is the desktop's entry and a fix to one is a fix to both.

use crate::grid::model::{GridItem, PanelKind};
use crate::main_menu::{FileMenu, HelpMenu, OptionsMenu};
use crate::panels::dashboards::{surfaces, title_of, DashboardConfig};
use crate::panels::incarnate_picker::IncarnatePickerOpen;
use crate::popover::PopoverInline;
use crate::quickbar::{model::QuickBarItem, DisplayMenu, ToolsMenu};
use crate::shell::{CombatPopover, Db};
use crate::view::marks;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sheet {
    Identity,
    Stats,
    More,
}

/// Which sheet is open, shared by the shell so the build bar can open one too.
#[derive(Clone, Copy)]
pub(crate) struct OpenSheet(pub Signal<Option<Sheet>>);

impl Sheet {
    fn title(self) -> &'static str {
        match self {
            Sheet::Identity => "Build Identity",
            Sheet::Stats => "Stats",
            Sheet::More => "More",
        }
    }
}

#[component]
pub fn MobileNav(
    database: Option<Db>,
    dataset: Signal<coh_data::DatasetId>,
    pins: Signal<Vec<QuickBarItem>>,
    layout: Signal<Vec<GridItem>>,
    reorder_open: Signal<bool>,
    reset_confirm_open: Signal<bool>,
) -> Element {
    let mut sheet = use_context::<OpenSheet>().0;
    let mut incarnate = use_context::<IncarnatePickerOpen>().0;
    // The picker opens on its first tab; it has the slot tabs to move between the rest. `None`
    // while the bundle loads or on a dataset with no incarnates, and More's entry is disabled then.
    let first_slot = database.as_ref().and_then(|db| {
        db.offered_incarnate_slots()
            .first()
            .map(|slot| slot.id.clone())
    });

    let mut switch_to = move |next: Sheet| {
        incarnate.set(None);
        sheet.set(if sheet() == Some(next) {
            None
        } else {
            Some(next)
        });
    };
    let build_active = sheet().is_none() && incarnate().is_none();

    rsx! {
        nav {
            // Raised over the modal layer while the incarnate picker is open, so the bar stays
            // live over it and any tab closes it — the beta's bar did the same over its incarnate
            // modal. Only for that picker: any other modal is a task the bar has no part in.
            class: if incarnate().is_some() { "mobile-nav mobile-nav--over-modal" } else { "mobile-nav" },
            "aria-label": "Mobile",
            if let Some(open) = sheet() {
                MobileSheet { key: "{open.title()}", sheet, title: open.title(),
                    match open {
                        // The form itself rather than its popover: a popover drawn inline
                        // would repeat the sheet's title as its own heading.
                        Sheet::Identity => rsx! {
                            if let Some(database) = database.clone() {
                                div { class: "mobile-sheet__panel",
                                    crate::panels::identity::IdentityPanel { database, dataset }
                                }
                            }
                        },
                        Sheet::Stats => rsx! { StatsSheet {} },
                        Sheet::More => rsx! {
                            FileMenu { database: database.clone(), dataset }
                            CombatPopover { database: database.clone() }
                            DisplayMenu { pins, layout, reorder_open, reset_confirm_open }
                            IncarnateEntry { first_slot: first_slot.clone(), sheet }
                            ToolsMenu { pins, layout, database: database.clone() }
                            OptionsMenu {}
                            HelpMenu {}
                        },
                    }
                }
            }
            NavTab {
                label: "Build",
                active: build_active,
                onclick: move |_| {
                    if build_active {
                        let _ = document::eval("window.scrollTo({ top: 0, behavior: 'smooth' });");
                    }
                    sheet.set(None);
                    incarnate.set(None);
                },
                {marks::home()}
            }
            NavTab {
                label: "Stats",
                active: sheet() == Some(Sheet::Stats),
                onclick: move |_| switch_to(Sheet::Stats),
                {marks::dashboard()}
            }
            NavTab {
                label: "More",
                // The incarnate picker is reached from More, so More stays lit while it is open.
                active: sheet() == Some(Sheet::More) || incarnate().is_some(),
                onclick: move |_| switch_to(Sheet::More),
                {marks::more()}
            }
        }
    }
}

/// More's way into the incarnate picker. It closes the sheet first, so the picker (a modal)
/// is not opened behind it.
#[component]
fn IncarnateEntry(first_slot: Option<String>, sheet: Signal<Option<Sheet>>) -> Element {
    let mut incarnate = use_context::<IncarnatePickerOpen>().0;
    let disabled = first_slot.is_none();
    rsx! {
        section { class: "mobile-sheet__panel",
            h3 { class: "popover-inline__title", "Incarnate" }
            div { class: "main-menu",
                button {
                    class: "main-menu__item",
                    r#type: "button",
                    disabled,
                    onclick: move |_| {
                        sheet.set(None);
                        incarnate.set(first_slot.clone());
                    },
                    span { class: "main-menu__label", "Incarnate powers…" }
                    span { class: "main-menu__hint",
                        if disabled { "This server has no incarnate powers" } else { "Alpha, Judgement, Interface and the rest" }
                    }
                }
            }
        }
    }
}

#[component]
fn NavTab(
    label: &'static str,
    active: bool,
    #[props(default)] disabled: bool,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: if active { "mobile-nav__tab is-active" } else { "mobile-nav__tab" },
            r#type: "button",
            "aria-pressed": active,
            disabled,
            onclick: move |evt| onclick.call(evt),
            {children}
            span { class: "mobile-nav__label", "{label}" }
        }
    }
}

/// The sheet: everything above the bar, a title row, and a scrolling body.
///
/// It lives inside the bar, which is `position: fixed` and so a stacking context at z 100. That
/// puts the sheet over the sticky mobile header (z 40) and under every modal (z 1000), so a
/// picker or a confirm raised from inside a sheet opens over it rather than behind.
#[component]
fn MobileSheet(sheet: Signal<Option<Sheet>>, title: &'static str, children: Element) -> Element {
    // The popovers listed in this sheet take this as their open state. A menu entry that acts
    // closes its menu, and here that has to close the sheet, or the sheet would stand over the
    // dialog the entry just raised.
    let open = use_signal(|| true);
    use_context_provider(|| PopoverInline(open));
    use_effect(move || {
        if !open() {
            sheet.set(None);
        }
    });

    rsx! {
        div {
            class: "mobile-sheet",
            role: "dialog",
            "aria-label": "{title}",
            tabindex: "-1",
            onmounted: move |evt| async move {
                let _ = evt.set_focus(true).await;
            },
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    sheet.set(None);
                }
            },
            header { class: "mobile-sheet__head",
                h2 { class: "mobile-sheet__title", "{title}" }
                button {
                    class: "mobile-sheet__close",
                    r#type: "button",
                    "aria-label": "Close",
                    onclick: move |_| sheet.set(None),
                    {marks::close()}
                }
            }
            div { class: "mobile-sheet__body", {children} }
        }
    }
}

/// Every stat panel the user has built, in their order, whether or not it is shown in the
/// stack — this tab is where the numbers are read, so a panel hidden to shorten the stack is
/// still one tap away.
#[component]
fn StatsSheet() -> Element {
    let config = use_context::<DashboardConfig>().0;
    let panels: Vec<(String, PanelKind)> = surfaces(&config.read())
        .into_iter()
        .filter(|panel| matches!(panel, PanelKind::Dashboard(_)))
        .map(|panel| (title_of(panel, &config.read()), panel))
        .collect();

    rsx! {
        for (index, (title, panel)) in panels.into_iter().enumerate() {
            if let PanelKind::Dashboard(id) = panel {
                section { key: "{index}", class: "mobile-sheet__panel",
                    h3 { class: "popover-inline__title", "{title}" }
                    crate::panels::stats::StatGroup { panel: id }
                }
            }
        }
    }
}
