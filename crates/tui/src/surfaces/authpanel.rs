//! The sign-in panel — the auth surface's own look, distinct from the
//! generic picker: three-space indented prose rows, choices marked with a
//! `› ` caret (the one place a caret appears), descriptions at an absolute
//! column, and per-stage rows. Everything dim except the selected choice or
//! the live input row.
//!
//! Stages: Choose (account vs API key, the reference flow's wording) →
//! Account or Key (which provider, labeled by display name) → ApiKey (inline
//! `┃ •••` entry mirroring the composer) or Waiting (browser authorization
//! in flight) → Done (the outcome beat, returning to the list).
//!
//! Navigation contract: Backspace goes back one level (in the API-key entry
//! it deletes while there is text and only navigates when the input is
//! empty); Esc always closes the whole panel. Providers already signed in
//! are marked in the description column so a return visit shows the result.

use crate::markdown::visible_width;
use crate::render::{self, bold};
use crate::theme::Theme;
use e_core::auth::{self};
use e_core::providers::registry::{self, Provider};

pub enum AuthStage {
    /// The method choice; `selected` indexes the two options. The root:
    /// Esc closes the panel, Backspace has nowhere to go.
    Choose { selected: usize },
    /// Which subscription to sign in with.
    Account { selected: usize },
    /// Which provider the API key belongs to.
    Key { selected: usize },
    /// Key entry for a provider; the composer holds the (masked) secret.
    ApiKey { provider: String },
    /// Browser OAuth in flight. `back` is the account-list row that launched
    /// the flow, or None when it was launched by a direct `/login <provider>`
    /// command (canceling that returns nowhere — the panel closes).
    Waiting { back: Option<usize> },
    /// The flow's outcome beat: shows the result, then any key but Esc
    /// returns to the list the flow belongs to.
    Done {
        ok: bool,
        message: String,
        back: BackTarget,
    },
}

/// The row Up (`up`) or Down moves to in a list of `len` rows, wrapping at
/// either end.
pub fn step(selected: usize, len: usize, up: bool) -> usize {
    let len = len.max(1);
    if up {
        (selected + len - 1) % len
    } else {
        (selected + 1) % len
    }
}

/// Where a finished flow returns: the list it belongs to, selection preserved.
#[derive(Clone, Copy)]
pub enum BackTarget {
    Account(usize),
    Key(usize),
}

impl BackTarget {
    /// The stage this target returns to.
    pub fn stage(self) -> AuthStage {
        match self {
            BackTarget::Account(selected) => AuthStage::Account { selected },
            BackTarget::Key(selected) => AuthStage::Key { selected },
        }
    }
}

/// The narrow-frame floor for the description column, anchored to the
/// longest choice label the panel shows.
const DESCRIPTION_COL: usize = 34;

pub(crate) fn choice_row(
    theme: &Theme,
    selected: bool,
    label: &str,
    description: &str,
    width: usize,
) -> String {
    // The reference caret sits at column one — `› label` selected, a plain
    // two-space indent otherwise — and the selected row brightens its value
    // column along with its label.
    let caret = if selected { "› " } else { "  " };
    let head = format!("{caret}{label}");
    let description_col = (width * 2 / 3).max(DESCRIPTION_COL).min(width);
    let styled_head = if selected {
        bold(&theme.fg("userMessageText", &head))
    } else {
        theme.fg("dim", &head)
    };
    let mut row = styled_head;
    if width > description_col {
        let pad = description_col.saturating_sub(visible_width(&head));
        let tail = format!("{}{}", " ".repeat(pad), description);
        if selected {
            row.push_str(&bold(&theme.fg("userMessageText", &tail)));
        } else {
            row.push_str(&theme.fg("dim", &tail));
        }
    }
    row
}

/// The panel's rows for `stage`. `mask_count` is the masked key's length
/// in the API-key entry.
pub fn render(stage: &AuthStage, theme: &Theme, width: usize, mask_count: usize) -> Vec<String> {
    let dim = |s: &str| theme.fg("dim", s);
    match stage {
        AuthStage::Choose { selected } => choose_rows(theme, width, *selected),
        AuthStage::Account { selected } => provider_rows(
            theme,
            width,
            *selected,
            "   Sign in with an account",
            registry::oauth_providers(),
            |provider| &provider.auth.oauth_hint,
        ),
        AuthStage::Key { selected } => provider_rows(
            theme,
            width,
            *selected,
            "   Sign in with an API key",
            registry::key_providers(),
            |provider| &provider.auth.key_hint,
        ),
        AuthStage::ApiKey { provider } => api_key_rows(theme, width, provider, mask_count),
        AuthStage::Waiting { .. } => vec![
            String::new(),
            dim("   Sign in with an account"),
            String::new(),
            dim("   Waiting for authorization in the browser…"),
            dim(&format!(
                "   {} cancels sign-in · Esc Close",
                render::backspace_label()
            )),
        ],
        AuthStage::Done { ok, message, .. } => {
            let head = if *ok {
                bold(&theme.fg("userMessageText", "   Login successful"))
            } else {
                bold(&theme.fg("userMessageText", "   Sign-in failed"))
            };
            vec![
                String::new(),
                head,
                dim(&format!("   {message}")),
                String::new(),
                dim("   Enter Continue · Esc Close"),
            ]
        }
    }
}

/// The root: account or API key, with where the key is stored.
fn choose_rows(theme: &Theme, width: usize, selected: usize) -> Vec<String> {
    vec![
        String::new(),
        theme.fg("dim", "   Sign in"),
        String::new(),
        choice_row(
            theme,
            selected == 0,
            "Sign in with an account",
            "subscription — opens the browser",
            width,
        ),
        choice_row(
            theme,
            selected == 1,
            "Sign in with an API key",
            if e_core::CHANNEL == "production" {
                "stored in ~/.e/auth.json"
            } else {
                "stored in this channel's auth.json"
            },
            width,
        ),
        String::new(),
        theme.fg("dim", "   ↑↓ Choose · Enter Continue · Esc Cancel"),
    ]
}

/// A provider list under `title`: each provider by display name, described
/// by `hint` unless it is already signed in.
fn provider_rows(
    theme: &Theme,
    width: usize,
    selected: usize,
    title: &str,
    providers: Vec<&'static Provider>,
    hint: fn(&'static Provider) -> &'static str,
) -> Vec<String> {
    let mut rows = vec![String::new(), theme.fg("dim", title), String::new()];
    let auth = auth::load();
    for (i, provider) in providers.into_iter().enumerate() {
        let description = if auth::signed_in(&auth, &provider.name) {
            "signed in"
        } else {
            hint(provider)
        };
        rows.push(choice_row(
            theme,
            selected == i,
            &provider.display,
            description,
            width,
        ));
    }
    rows.push(String::new());
    rows.push(theme.fg(
        "dim",
        &format!(
            "   ↑↓ Choose · Enter Continue · {} Back · Esc Close",
            render::backspace_label(),
        ),
    ));
    rows
}

/// Key entry: the `┃` rail over a placeholder, or one `•` per typed char.
fn api_key_rows(theme: &Theme, width: usize, provider: &str, mask_count: usize) -> Vec<String> {
    let dim = |s: &str| theme.fg("dim", s);
    let entry = if mask_count == 0 {
        format!(
            "{}{}",
            bold(&theme.fg("userMessageText", "   ┃ ")),
            dim("Paste or type a key")
        )
    } else {
        bold(&theme.fg(
            "userMessageText",
            &format!(
                "   ┃ {}",
                "•".repeat(mask_count.min(width.saturating_sub(6)))
            ),
        ))
    };
    vec![
        String::new(),
        dim(&format!(
            "   Paste your {} API key",
            e_core::providers::catalog::display_name(provider)
        )),
        entry,
        dim(&format!(
            "   Enter saves · {} Back · Esc Close",
            render::backspace_label()
        )),
    ]
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn up_and_down_move_opposite_ways_and_wrap() {
        assert_eq!(step(0, 3, false), 1);
        assert_eq!(step(2, 3, false), 0);
        assert_eq!(step(1, 3, true), 0);
        assert_eq!(step(0, 3, true), 2);
        assert_eq!(step(0, 0, true), 0);
    }
}
