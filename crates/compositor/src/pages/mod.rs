// Generated built-in pages, served from memory at `spatial://page/…` via
// `serve` (never `data:` URLs).
// New and ported pages build on `ui` (maud components + ui.css themed by
// CSS variables + the shared ui.js keyboard model), which escapes every
// interpolated value itself. Legacy `format!` pages must still:
// Escape every interpolated value for its context: `html_escape` for HTML
// text/attributes, `js_string_literal` for JS strings (handlers and
// `<script>`), `url_component` for `scheme://…?k=v` values.

use crate::output::Theme;

pub mod bookmarks_list;
pub mod context_menu;
pub mod downloads_list;
pub mod find_bar;
#[cfg(test)]
mod gallery;
pub mod help;
pub mod history_list;
pub mod omnibox;
pub mod palette;
pub mod passwords_list;
pub mod save_prompt;
pub mod settings_list;
pub mod ui;
pub mod userscripts_list;
pub mod workspace_list;

/// Built-in page kinds — the `<kind>` in `spatial://page/<kind>/<token>`.
pub(crate) mod kind {
    /// Empty page a pre-started browser idles on (browser::warm pool).
    pub const BLANK: &str = "blank";
    pub const BOOKMARKS: &str = "bookmarks";
    pub const CONTEXT_MENU: &str = "context-menu";
    pub const DOWNLOADS: &str = "downloads";
    pub const FIND_BAR: &str = "find-bar";
    pub const HELP: &str = "help";
    pub const HISTORY: &str = "history";
    pub const OMNIBOX: &str = "omnibox";
    pub const PALETTE: &str = "palette";
    pub const SAVE_PROMPT: &str = "save-prompt";
    pub const SETTINGS: &str = "settings";
    pub const USERSCRIPTS: &str = "userscripts";
    pub const VAULT_LIST: &str = "vault-list";
    pub const VAULT_UNLOCK: &str = "vault-unlock";
    pub const WORKSPACES: &str = "workspaces";
}

/// Keeps a list's scroll position across refresh-in-place: remembers
/// `scrollY` per kind and `data-view` (settings/passwords tab) in the
/// tile's sessionStorage and restores it after `load` (after the list
/// keyboard script has scrolled row 0 into view).
const SCROLL_KEEP_SCRIPT: &str = r#"<script>(function(){
var s;try{s=window.sessionStorage;}catch(e){return;}
if(!s)return;
var key='sb-scroll:'+(location.pathname.split('/')[1]||'')+':'+((document.body&&document.body.dataset.view)||'');
addEventListener('scroll',function(){try{s.setItem(key,String(Math.round(scrollY)));}catch(e){}},{passive:true});
addEventListener('load',function(){var y=0;try{y=+s.getItem(key)||0;}catch(e){}if(y>0)scrollTo(0,y);});
})();</script>"#;

/// Serve a built-in page's HTML at a fresh `spatial://page/<kind>/<token>`
/// URL (see `cef_bridge::spatial`). The token is 128 random bits: the
/// capability that lets the tile load it and makes its actions internal.
/// The HTML is served byte for byte — no data: URL decoding, no URL length
/// cap — under a strict CSP.
pub(crate) fn serve(kind: &str, mut html: String) -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    html.push_str(SCROLL_KEEP_SCRIPT);
    cef_bridge::spatial::publish(kind, &token, html).unwrap_or_else(|| {
        log::error!("invalid built-in page kind {kind:?}");
        "about:blank".to_string()
    })
}

/// Kind of a built-in page URL (`None` for anything else).
pub(crate) fn kind_of(url: &str) -> Option<&str> {
    cef_bridge::spatial::parse_page_url(url).map(|(kind, _)| kind)
}

/// Escape for HTML text and for double- or single-quoted attribute values.
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Body of a JS single-quoted string. Every ASCII char outside a small safe
/// set becomes `\uXXXX`, so the result cannot end the literal (`'`, `\`,
/// line breaks), close a `<script>` (`<`), or break out of an HTML
/// attribute (`"`, `'`, `&`) — valid in an inline handler and in a
/// `<script>` element alike.
fn js_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | ' '
            | '-'
            | '_'
            | '.'
            | ','
            | ':'
            | '/'
            | '@'
            | '?'
            | '='
            | '+'
            | '*'
            | '('
            | ')'
            | '['
            | ']'
            | '!'
            | '$'
            | '~'
            | ';' => out.push(c),
            c if c.is_ascii() || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// JS single-quoted literal, safe in an inline handler attribute
/// (`onclick="go({lit})"`) and inside a `<script>` element.
pub(crate) fn js_string_literal(s: &str) -> String {
    format!("'{}'", js_escape(s))
}

/// Percent-encode everything but RFC 3986 unreserved bytes — for values
/// spliced into `scheme://…?k=<v>` hrefs. Output is `[A-Za-z0-9-_.~%]`
/// only, so it is also HTML-attribute- and JS-string-safe as is.
pub(crate) fn url_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// ArrowUp/Down/Enter for `.list-row[data-open]`; skips focused `<input>`.
pub(crate) const LIST_NAV_SCRIPT: &str = r#"<script>
let listActiveIndex = 0;
function listRows() {
  return Array.from(document.querySelectorAll('.list-row')).filter(r => r.offsetParent !== null);
}
function setListActive(index) {
  const rows = listRows();
  if (!rows.length) return;
  listActiveIndex = ((index % rows.length) + rows.length) % rows.length;
  rows.forEach(r => r.classList.remove('list-active'));
  rows[listActiveIndex].classList.add('list-active');
  rows[listActiveIndex].scrollIntoView({block: 'nearest'});
}
document.addEventListener('keydown', (event) => {
  if (event.target.tagName === 'INPUT') return;
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    setListActive(listActiveIndex + 1);
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    setListActive(listActiveIndex - 1);
  } else if (event.key === 'Enter') {
    const row = listRows()[listActiveIndex];
    if (row && row.dataset.open) window.location = row.dataset.open;
  }
});
document.addEventListener('mouseover', (event) => {
  const row = event.target.closest('.list-row');
  if (!row) return;
  const index = listRows().indexOf(row);
  if (index >= 0) setListActive(index);
});
window.addEventListener('load', () => setListActive(0));
</script>"#;

pub(crate) fn body_open(theme: &Theme, extra_attrs: &str) -> String {
    format!(
        "<body {extra_attrs} style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// Empty-list placeholder (`text` is app-authored markup, not escaped).
pub(crate) fn empty_state(theme: &Theme, text: &str) -> String {
    format!(
        "<p style=\"color:{fg};opacity:0.7\">{text}</p>",
        fg = theme.help_fg
    )
}

/// Favicon tile: the locally cached icon (captured on visit, see
/// persistence::favicons) over a letter fallback (`letter` already
/// uppercased, escaped here). Never fetches from `host`.
pub(crate) fn favicon_tile(host: &str, letter: &str, theme: &Theme) -> String {
    let letter = html_escape(letter);
    // A data:image/png;base64 URI: no quotes or markup to escape.
    let icon = crate::persistence::favicons::data_uri(host)
        .map(|uri| {
            format!(
                "<img src=\"{uri}\" width=\"20\" height=\"20\" \
                 style=\"position:absolute;inset:0;border-radius:4px\">"
            )
        })
        .unwrap_or_default();
    format!(
        "<span style=\"position:relative;width:20px;height:20px;flex-shrink:0\">\
         <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
         color:{key_fg};display:flex;align-items:center;justify-content:center;\
         font-size:11px;font-weight:700\">{letter}</span>{icon}</span>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}

pub(crate) const TRASH_SVG_PATH: &str =
    "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z";

pub(crate) const CHECKMARK_SVG_PATH: &str = "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z";

/// Per-row icon `<a href>` (`extra_attrs` e.g. stopPropagation on delete).
pub(crate) fn icon_link_button(
    href: &str,
    title: &str,
    svg_path: &str,
    extra_attrs: &str,
    theme: &Theme,
) -> String {
    format!(
        "<a href=\"{href}\" title=\"{title}\" class=\"bm-icon-btn\" {extra_attrs} \
         style=\"flex-shrink:0;margin-left:4px;display:flex;align-items:center;\
         justify-content:center;width:26px;height:26px;border-radius:6px;\
         background:{bg};color:{fg};opacity:0.7;text-decoration:none\">\
         <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
         <path d=\"{svg_path}\"/></svg></a>",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// Like `icon_link_button`, but a form submit (rename-save).
pub(crate) fn icon_submit_button(title: &str, svg_path: &str, theme: &Theme) -> String {
    format!(
        "<button type=\"submit\" title=\"{title}\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
         margin-left:4px;display:flex;align-items:center;justify-content:center;width:26px;\
         height:26px;border-radius:6px;background:{bg};color:{fg};opacity:0.7;border:none;\
         cursor:pointer;font:inherit\">\
         <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
         <path d=\"{svg_path}\"/></svg></button>",
        bg = theme.help_bg,
        fg = theme.help_fg,
    )
}

/// `.bm-icon-btn:hover` rule body for splicing into page `<style>`.
pub(crate) fn icon_button_hover_css(theme: &Theme) -> String {
    format!(
        ".bm-icon-btn:hover{{background:{key_bg}!important;color:{key_fg}!important;\
         opacity:1!important}}",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::output::THEMES;

    /// The HTML a fresh (unbound) built-in page URL serves.
    pub(crate) fn served_html(url: &str) -> String {
        let body = cef_bridge::spatial::body(url, None).expect("served page");
        String::from_utf8(body.as_bytes().to_vec()).expect("utf-8")
    }

    #[test]
    fn serve_keeps_hostile_bodies_byte_for_byte() {
        for html in [
            "",
            "<a href=\"context://open-link?url=%22%3E%3Cimg src=x onerror=alert(1)%3E\">x</a>",
            "100% <b>#fragment</b> %2523 %",
            "Привіт, світ — ünïcødé 🚚",
            "line\nbreak\ttab\r\n",
        ] {
            let url = serve(kind::HELP, html.to_string());
            assert!(url.starts_with("spatial://page/help/"), "{url}");
            let served = served_html(&url);
            assert_eq!(&served[..html.len()], html);
            assert_eq!(&served[html.len()..], SCROLL_KEEP_SCRIPT);
        }
    }

    #[test]
    fn serve_issues_unique_well_formed_tokens() {
        let a = serve(kind::HELP, String::new());
        let b = serve(kind::HELP, String::new());
        assert_ne!(a, b);
        for url in [&a, &b] {
            let (k, token) = cef_bridge::spatial::parse_page_url(url).expect("parses");
            assert_eq!(k, kind::HELP);
            assert_eq!(token.len(), 32);
        }
        assert_eq!(kind_of(&a), Some(kind::HELP));
        assert_eq!(kind_of("data:text/html,x"), None);
        // A bad kind never yields a servable URL.
        assert_eq!(serve("Not A Kind", String::new()), "about:blank");
    }

    #[test]
    fn every_page_is_a_spatial_url() {
        use crate::persistence::{downloads::DownloadRecord, history::HistoryEntry};
        let theme = THEMES[0];
        let workspaces: crate::persistence::workspaces::WorkspaceStore =
            serde_json::from_str(r#"{"active":0,"slots":[]}"#).unwrap();
        let pages = [
            (kind::HELP, help::page_url(&theme)),
            (
                kind::PALETTE,
                palette::page_url(
                    &theme,
                    &[palette::OpenPage {
                        id: 1,
                        url: "https://a.test/#x".into(),
                        title: None,
                        current: true,
                    }],
                    &[],
                    &[],
                    "https://e.test/?q=",
                ),
            ),
            (
                kind::OMNIBOX,
                omnibox::page_url(&theme, &["#1".into()], "https://e.test/?q="),
            ),
            (
                kind::VAULT_UNLOCK,
                passwords_list::unlock_url(&theme, true, Some("#err")),
            ),
            (
                kind::VAULT_LIST,
                passwords_list::page_url(&theme, &[], &[], Some("gen"), None, "generator"),
            ),
            (kind::FIND_BAR, find_bar::page_url(&theme)),
            (
                kind::SAVE_PROMPT,
                save_prompt::page_url(&theme, "a.test", "u", true),
            ),
            (
                kind::CONTEXT_MENU,
                context_menu::page_url(&theme, &Default::default()),
            ),
            (
                kind::BOOKMARKS,
                bookmarks_list::page_url(&theme, &[], Some("ok")),
            ),
            (
                kind::DOWNLOADS,
                downloads_list::page_url(
                    &theme,
                    &[DownloadRecord {
                        url: "https://a.test/f".into(),
                        path: "/tmp/f".into(),
                        mime: None,
                    }],
                    &[],
                ),
            ),
            (
                kind::HISTORY,
                history_list::page_url(
                    &theme,
                    &[HistoryEntry {
                        url: "https://a.test/".into(),
                        visited_at: 0,
                        title: Some("A".into()),
                    }],
                ),
            ),
            (
                kind::WORKSPACES,
                workspace_list::page_url(&theme, &workspaces),
            ),
            (
                kind::SETTINGS,
                settings_list::page_url(&theme, &Default::default()),
            ),
            (
                kind::USERSCRIPTS,
                userscripts_list::page_url(&theme, &[], &[]),
            ),
        ];
        for (expected, url) in pages {
            assert_eq!(kind_of(&url), Some(expected), "{url}");
            assert!(served_html(&url).contains("</body>"), "{expected}");
        }
    }

    #[test]
    fn list_views_tag_their_tab_for_scroll_memory() {
        let theme = THEMES[0];
        let settings = settings_list::page_url(&theme, &Default::default());
        assert!(served_html(&settings).contains("data-view=\"general\""));
        let vault = passwords_list::page_url(&theme, &[], &[], None, None, "never");
        assert!(served_html(&vault).contains("data-view=\"never\""));
    }

    #[test]
    fn escapers_neutralise_markup() {
        assert_eq!(
            html_escape(r#"<img src=x onerror='a()'>&"#),
            "&lt;img src=x onerror=&#39;a()&#39;&gt;&amp;"
        );
        assert_eq!(
            js_string_literal("x';alert(1)//\"</script>\n\u{2028}é"),
            "'x\\u0027;alert(1)//\\u0022\\u003c/script\\u003e\\u000a\\u2028é'"
        );
        assert_eq!(url_component("a b&c=\"<'#%"), "a%20b%26c%3D%22%3C%27%23%25");
    }

    #[test]
    fn favicon_tile_escapes_host_and_letter() {
        let html = favicon_tile("x\"><script>", "<", &THEMES[0]);
        assert!(!html.contains("<script>"));
        assert!(html.contains(">&lt;</span>"));
        // No icon cached for this host, and never a request to it.
        assert!(!html.contains("<img"));
        assert!(!html.contains("favicon.ico"));
    }

    #[test]
    fn context_menu_link_cannot_inject_markup() {
        // Delivered by any web page via `context://hit?link=…`.
        let ctx = context_menu::MenuContext {
            link: Some("https://x.test/%22%3E%3Cimg src=x onerror=alert(1)%3E\"><b>".into()),
            ..Default::default()
        };
        let html = served_html(&context_menu::page_url(&THEMES[0], &ctx));
        assert!(!html.contains("<img") && !html.contains("<b>"), "{html}");
        assert!(html.contains("context://open-link?url=https%3A%2F%2Fx.test%2F%2522%253E"));
    }

    #[test]
    fn passwords_list_escapes_web_controlled_fields() {
        use crate::persistence::vault::test_entry;
        let mut e = test_entry("id'); x('", "https://a.test\"><script>");
        e.username = "<img src=x onerror=1>".into();
        let never = vec!["https://n.test');alert(1);('".to_string()];
        let saved = served_html(&passwords_list::page_url(
            &THEMES[0],
            &[e],
            &never,
            None,
            None,
            "saved",
        ));
        assert!(
            !saved.contains("<img") && !saved.contains("\"><script>"),
            "{saved}"
        );
        assert!(saved.contains("a.test&quot;&gt;&lt;script&gt;"));
        assert!(saved.contains("password://go/fill?id=id%27%29%3B%20x%28%27"));
        let never_page = served_html(&passwords_list::page_url(
            &THEMES[0],
            &[],
            &never,
            None,
            None,
            "never",
        ));
        assert!(!never_page.contains("');alert(1)"), "{never_page}");
        assert!(never_page.contains("remove-never?origin=https%3A%2F%2Fn.test%27%29%3Balert"));
    }
}
