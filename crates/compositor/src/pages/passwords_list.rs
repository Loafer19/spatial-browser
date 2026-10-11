// Ctrl+Shift+P vault UI (`password://…`) on the ui design system. Only the
// built-in vault tiles may send these actions (see pending_actions); the
// web-page autofill subset lives elsewhere and is unchanged.

use super::ui::{self, html, Icon, Markup};
use super::url_component;
use crate::output::Theme;
use crate::persistence::vault::{self, VaultEntry};

/// Id of the list page heading (styling/userstyle hook only; detection
/// goes by the URL's kind).
const LIST_MARKER: &str = "sb-vault-list";

/// Whether a page URL is the unlocked passwords list (shows entries, so it
/// must be swapped for the unlock form on lock).
pub(crate) fn is_list_url(url: &str) -> bool {
    super::kind_of(url) == Some(super::kind::VAULT_LIST)
}

const TABS: &[(&str, &str, &str)] = &[
    ("saved", "Saved", "password://go/tab/saved"),
    ("add", "Add / Import", "password://go/tab/add"),
    ("generator", "Generator", "password://go/tab/generator"),
    ("never", "Never save", "password://go/tab/never"),
];

pub(crate) fn normalize_tab(tab: &str) -> &'static str {
    TABS.iter()
        .find(|(id, _, _)| *id == tab)
        .map_or("saved", |(id, _, _)| id)
}

const UNLOCK_CSS: &str = r#"
.unlock { max-width: 420px; margin: 10vh auto 0; }
.unlock .card { padding: 22px; }
.unlock label { display: block; margin: 12px 0 4px; }
.unlock .input { width: 100%; }
.unlock .btn { margin-top: 16px; }
#err { min-height: 1.3em; margin: 8px 0 0; }
"#;

const UNLOCK_SCRIPT: &str = r#"
document.getElementById('vault-form').addEventListener('submit', function (ev) {
  ev.preventDefault();
  var err = document.getElementById('err');
  err.textContent = '';
  var create = this.dataset.create === '1';
  var p = this.password.value;
  var c = this.confirm ? this.confirm.value : '';
  if (create) {
    if (p.length < 8) { err.textContent = 'Use at least 8 characters'; return; }
    if (c !== p) { err.textContent = 'Passwords do not match'; return; }
  }
  location = 'password://go/unlock?create=' + (create ? 1 : 0) + '&password=' + encodeURIComponent(p) +
    (c ? '&confirm=' + encodeURIComponent(c) : '');
});
"#;

pub(crate) fn unlock_url(theme: &Theme, create: bool, error: Option<&str>) -> String {
    let heading = if create {
        "Create password vault"
    } else {
        "Unlock password vault"
    };
    let body = html! {
        main class="page unlock" {
            div class="card" {
                h1 { (Icon::Lock) " " (heading) }
                p class="small muted" {
                    "Local encrypted vault (AES-GCM). Locks after "
                    (vault::AUTO_LOCK_AFTER.as_secs() / 60) " min idle or on " kbd { "Ctrl+Shift+L" } "."
                }
                form id="vault-form" data-create=(if create { "1" } else { "0" }) {
                    label class="small" for="pw" { "Master password" }
                    input id="pw" class="input" name="password" type="password" required autofocus
                        autocomplete=(if create { "new-password" } else { "current-password" });
                    @if create {
                        label class="small" for="pw2" { "Confirm" }
                        input id="pw2" class="input" name="confirm" type="password" required autocomplete="new-password";
                    }
                    p id="err" class="small danger-text" role="alert" { (error.unwrap_or("")) }
                    button class="btn primary" type="submit" { (if create { "Create vault" } else { "Unlock" }) }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, heading)
        .no_row()
        .css(UNLOCK_CSS)
        .script(UNLOCK_SCRIPT)
        .render(body);
    super::serve(super::kind::VAULT_UNLOCK, html)
}

const CSS: &str = r#"
.form { display: grid; gap: 8px; max-width: 460px; }
.form .btn { justify-self: start; }
.generated { font-family: var(--mono); word-break: break-all; padding: 10px 12px; margin: 10px 0; }
"#;

// The add form builds its action by hand so values are encoded the same
// way the parser expects (never `+` for spaces).
const SCRIPT: &str = r#"
(function () {
  var f = document.getElementById('add-form');
  if (!f) return;
  f.addEventListener('submit', function (e) {
    e.preventDefault();
    ui.go('password://go/upsert?origin=' + encodeURIComponent(f.origin.value) +
      '&username=' + encodeURIComponent(f.username.value) +
      '&password=' + encodeURIComponent(f.password.value) +
      '&email=' + encodeURIComponent(f.email.value));
  });
})();
"#;

fn saved(entries: &[VaultEntry]) -> Markup {
    html! {
        @if entries.is_empty() {
            (ui::empty(Icon::Key, html! { "No saved logins yet. Use " b { "Add / Import" } ", or accept a save prompt on a site." }))
        } @else {
            div class="cluster" style="margin-bottom:10px" {
                (ui::search("q", "", "Filter logins", Some(".list")))
            }
        }
        div class="list" {
            @for e in entries {
                // Ids come from CSV imports too — percent-encode, never splice raw.
                @let fill = format!("password://go/fill?id={}", url_component(&e.id));
                @let delete = format!("password://go/delete?id={}", url_component(&e.id));
                @let host = display_host(&e.origin);
                @let email = e.email.as_deref().filter(|s| !s.is_empty());
                div class="row card" data-open=(fill) data-delete=(delete)
                    data-confirm=(format!("Delete login for {host}"))
                    data-text=(format!("{} {} {}", e.origin, e.username, email.unwrap_or(""))) title=(e.origin) {
                    (ui::favicon(host, host))
                    div class="main" {
                        span class="title" { (host) }
                        @if !e.username.is_empty() { span class="sub" { (e.username) } }
                        @if let Some(email) = email { span class="sub" { (email) } }
                    }
                    div class="actions" {
                        a class="btn sm primary" href=(fill) { "Fill" }
                        (ui::icon_link(&delete, "Delete", Icon::Trash, true))
                    }
                }
            }
        }
        div class="empty hidden" data-filter-empty { (Icon::Search) div { "No login matches." } }
    }
}

fn add() -> Markup {
    html! {
        h2 { "Import CSV" }
        p class="small muted" { "Chrome or Bitwarden export — opens a system file dialog." }
        a class="btn primary sm" href="password://go/import-browse" { (Icon::Download) "Choose CSV file…" }
        h2 { "Add entry" }
        form id="add-form" class="form" {
            input class="input" name="origin" required placeholder="https://example.com" aria-label="Site";
            input class="input" name="username" placeholder="Username" aria-label="Username" autocomplete="off";
            input class="input" name="password" type="password" required placeholder="Password" aria-label="Password" autocomplete="new-password";
            input class="input" name="email" placeholder="Email (optional)" aria-label="Email" autocomplete="off";
            button class="btn primary sm" type="submit" { (Icon::Check) "Save" }
        }
    }
}

fn generator(generated: Option<&str>) -> Markup {
    html! {
        p class="small muted" { "20 characters with symbols, from the OS random generator." }
        a class="btn primary sm" href="password://go/generate?length=20&symbols=1" { (Icon::Refresh) "Generate" }
        @if let Some(g) = generated {
            div class="card generated" { (g) }
            a class="btn sm" href="password://go/copy-generated" { (Icon::Copy) "Copy" }
        }
    }
}

fn never(never_save: &[String]) -> Markup {
    html! {
        p class="small muted" { "Sites where the save prompt never appears." }
        @if never_save.is_empty() {
            (ui::empty(Icon::Shield, html! { "None" }))
        }
        div class="list" {
            @for o in never_save {
                @let remove = format!("password://go/remove-never?origin={}", url_component(o));
                div class="row card" data-nav data-delete=(remove) title=(o) {
                    span class="main" { span class="title" { (o) } }
                    div class="actions" { (ui::icon_link(&remove, "Remove", Icon::Trash, true)) }
                }
            }
        }
    }
}

pub(crate) fn page_url(
    theme: &Theme,
    entries: &[VaultEntry],
    never_save: &[String],
    generated: Option<&str>,
    status: Option<&str>,
    active_tab: &str,
) -> String {
    let active_tab = normalize_tab(active_tab);
    let tabs: Vec<ui::Tab> = TABS
        .iter()
        .map(|(id, label, href)| ui::Tab {
            label,
            href,
            active: *id == active_tab,
            count: match *id {
                "saved" => Some(entries.len()),
                "never" => Some(never_save.len()),
                _ => None,
            },
        })
        .collect();
    let body = html! {
        main class="page" {
            header class="page-head" {
                h1 id=(LIST_MARKER) { "Passwords" }
                a class="btn sm" href="password://go/lock" title="Lock vault (Ctrl+Shift+L)" { (Icon::Lock) "Lock" }
                div class="sub small muted" { "Local vault · focus a login field on a site for a fill suggestion" }
            }
            @if let Some(status) = status.filter(|s| !s.is_empty()) {
                (ui::banner(ui::Tone::Info, status))
            }
            (ui::tabs(&tabs))
            @match active_tab {
                "add" => (add()),
                "generator" => (generator(generated)),
                "never" => (never(never_save)),
                _ => (saved(entries)),
            }
        }
    };
    let html = ui::Doc::new(theme, "Passwords")
        .view(active_tab)
        .keys(&[
            ("Enter", "Fill selected login"),
            ("Del Del", "Delete selected"),
        ])
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::VAULT_LIST, html)
}

/// Host(+port) for list display — strip `https://` / `http://`.
fn display_host(origin: &str) -> &str {
    origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin)
}

#[cfg(test)]
mod tag_tests {
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn list_page_is_detected_by_kind() {
        let list = super::page_url(&THEMES[0], &[], &[], None, None, "saved");
        assert!(list.starts_with("spatial://page/vault-list/"), "{list}");
        assert!(super::is_list_url(&list));
        let unlock = super::unlock_url(&THEMES[0], false, None);
        assert!(
            unlock.starts_with("spatial://page/vault-unlock/"),
            "{unlock}"
        );
        assert!(!super::is_list_url(&unlock));
        // The heading id alone (e.g. in a web page's URL) means nothing.
        assert!(!super::is_list_url("https://a.test/#sb-vault-list"));
        assert!(!super::is_list_url(
            "data:text/html;tag=sb-vault-list;base64,PGgxPg=="
        ));
    }

    #[test]
    fn entries_and_errors_are_escaped_and_ids_encoded() {
        let mut entry =
            crate::persistence::vault::test_entry("a&b=c", "https://x.test\"><img src=x>");
        entry.username = "<script>u()</script>".into();
        entry.password = "pw".into();
        let html = served_html(&super::page_url(
            &THEMES[0],
            &[entry],
            &["https://n.test/<b>".into()],
            Some("<i>gen</i>"),
            Some("<u>status</u>"),
            "saved",
        ));
        for raw in ["<img src=x", "<script>u()", "<u>status"] {
            assert!(!html.contains(raw), "{raw}");
        }
        assert!(html.contains("password://go/fill?id=a%26b%3Dc"));
        assert!(!html.contains(">pw<"), "passwords are never rendered");
        let unlock = served_html(&super::unlock_url(&THEMES[0], true, Some("<b>bad</b>")));
        assert!(!unlock.contains("<b>bad"));
        assert!(unlock.contains("data-create=\"1\""));
    }
}
