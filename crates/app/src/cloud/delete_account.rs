//! "Delete account…": the confirmation dialog in front of `delete-account`.
//!
//! The server erases the sign-in record, the profile, favourites, saved layout, every published
//! build and their preview images. There is no undo, so the button stays off until the person
//! types the word, and the server asks for the same word.

use super::account::Account;
use crate::modal::{Modal, ModalSize};
use dioxus::prelude::*;

/// What the person types to arm the button. The server checks for the same string.
const CONFIRM_WORD: &str = "DELETE";

/// Raised by the main menu's row; owned by the shell.
#[derive(Clone, Copy)]
pub struct DeleteAccountOpen(pub Signal<bool>);

/// Does what was typed arm the button? Surrounding spaces are forgiven, case is not.
pub fn confirms(typed: &str) -> bool {
    typed.trim() == CONFIRM_WORD
}

/// Mounted by the shell above both layout roots. Renders nothing while closed.
#[component]
pub fn DeleteAccountHost() -> Element {
    let mut open = use_context::<DeleteAccountOpen>().0;
    if !open() {
        return rsx! {};
    }
    rsx! {
        Modal {
            title: "Delete your account".to_string(),
            size: ModalSize::Md,
            on_close: move |_| open.set(false),
            DeleteAccountBody {}
        }
    }
}

#[component]
fn DeleteAccountBody() -> Element {
    let account = use_context::<Account>();
    let mut open = use_context::<DeleteAccountOpen>().0;
    let user = account.user();
    let mut typed = use_signal(String::new);
    let mut working = use_signal(|| false);
    let mut refusal = use_signal(|| None::<String>);

    // Signed out underneath the dialog (another tab, or a session that lapsed).
    if user.read().is_none() {
        return rsx! {
            div { class: "load-state", "You're not signed in, so there is no account to delete." }
        };
    }

    let armed = confirms(&typed()) && !working();
    let delete_now = {
        let account = account.clone();
        move |_| {
            let account = account.clone();
            async move {
                if working() {
                    return;
                }
                working.set(true);
                refusal.set(None);
                match account.delete_account().await {
                    Ok(()) => open.set(false),
                    Err(error) => refusal.set(Some(error.to_string())),
                }
                working.set(false);
            }
        }
    };

    rsx! {
        div { class: "sb-profile",
            p { "This permanently deletes your Sidekick account and everything stored with it:" }
            ul { class: "sb-delete__list",
                li { "every build you've published or saved to your account, and their share links" }
                li { "your public profile, handle and bio" }
                li { "your favourites and your saved layout" }
            }
            p { "It can't be undone. Builds saved as files on your own device aren't affected." }

            label { class: "field-label", "Type {CONFIRM_WORD} to confirm" }
            input {
                class: "sb-save__input",
                value: "{typed}",
                autocomplete: "off",
                spellcheck: "false",
                placeholder: CONFIRM_WORD,
                oninput: move |e| typed.set(e.value()),
            }

            if let Some(reason) = refusal() {
                p { class: "sb-save__error", "{reason}" }
            }

            div { class: "sb-detail__actions",
                button {
                    class: "seg",
                    r#type: "button",
                    onclick: move |_| open.set(false),
                    "Cancel"
                }
                button {
                    class: "seg is-danger",
                    r#type: "button",
                    disabled: !armed,
                    onclick: delete_now,
                    if working() { "Deleting…" } else { "Delete account" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::confirms;

    #[test]
    fn only_the_word_arms_the_button() {
        assert!(confirms("DELETE"));
        assert!(confirms("  DELETE "));
        assert!(!confirms("delete"));
        assert!(!confirms("DELET"));
        assert!(!confirms(""));
    }
}
