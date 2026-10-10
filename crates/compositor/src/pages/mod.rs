// Generated data: chrome pages. Every page's HTML goes through `data_url`
// (base64 body) — never splice raw HTML after `data:text/html,`.
// Escape every interpolated value for its context: `html_escape` for HTML
// text/attributes, `js_string_literal` for JS strings (handlers and
// `<script>`), `url_component` for `scheme://…?k=v` values.

use crate::output::Theme;

pub mod bookmarks_list;
pub mod context_menu;
pub mod downloads_list;
pub mod help;
pub mod history_list;
pub mod omnibox;
pub mod passwords_list;
pub mod settings_list;
pub mod switcher;
pub mod userscripts_list;
pub mod workspace_list;

/// Wrap a built-in page's HTML in a `data:` URL.
///
/// The body is base64, not raw or percent-encoded HTML. Chromium
/// percent-decodes a non-base64 `data:` body *before* the HTML parser sees
/// it, so with a raw body any `%22%3E%3Cimg …%3E` that survived
/// `html_escape` (which leaves `%` alone) turned back into live markup —
/// e.g. a web page's link URL in the context menu. A raw body also loses
/// everything after the first `#` (fragment start). Base64 has none of
/// these failure modes: the decoded bytes are exactly `html`, whatever it
/// contains, at ~1.33× size (percent-encoding every unsafe byte is up to
/// 3×). `charset=utf-8` stays — without it Chromium decodes as Latin-1.
pub(crate) fn data_url(html: &str) -> String {
    const PREFIX: &str = "data:text/html;charset=utf-8;base64,";
    let mut out = String::with_capacity(PREFIX.len() + html.len().div_ceil(3) * 4);
    out.push_str(PREFIX);
    base64_encode_into(html.as_bytes(), &mut out);
    out
}

/// Standard (RFC 4648 §4) padded base64.
fn base64_encode_into(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let sextet = |shift: u32| ALPHABET[((n >> shift) & 63) as usize] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
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
            'A'..='Z' | 'a'..='z' | '0'..='9' | ' ' | '-' | '_' | '.' | ',' | ':' | '/' | '@'
            | '?' | '=' | '+' | '*' | '(' | ')' | '[' | ']' | '!' | '$' | '~' | ';' => out.push(c),
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

/// Favicon tile with letter fallback (`letter` already uppercased). Both
/// come from page/bookmark URLs or titles — escaped here.
pub(crate) fn favicon_tile(host: &str, letter: &str, theme: &Theme) -> String {
    let host = html_escape(host);
    let letter = html_escape(letter);
    format!(
        "<span style=\"position:relative;width:20px;height:20px;flex-shrink:0\">\
         <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
         color:{key_fg};display:flex;align-items:center;justify-content:center;\
         font-size:11px;font-weight:700\">{letter}</span>\
         <img src=\"https://{host}/favicon.ico\" width=\"20\" height=\"20\" \
         style=\"position:absolute;inset:0;border-radius:4px\" \
         onerror=\"this.style.display='none'\"></span>",
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

    /// Decode a `data_url` back to its HTML (test-only base64 decoder).
    pub(crate) fn decode_data_url(url: &str) -> String {
        let b64 = url
            .strip_prefix("data:text/html;charset=utf-8;base64,")
            .expect("data_url prefix");
        let val = |c: u8| -> u32 {
            match c {
                b'A'..=b'Z' => u32::from(c - b'A'),
                b'a'..=b'z' => u32::from(c - b'a') + 26,
                b'0'..=b'9' => u32::from(c - b'0') + 52,
                b'+' => 62,
                b'/' => 63,
                _ => panic!("non-base64 byte {c:?}"),
            }
        };
        assert_eq!(b64.len() % 4, 0);
        let mut out = Vec::new();
        for chunk in b64.as_bytes().chunks(4) {
            let pad = chunk.iter().filter(|&&c| c == b'=').count();
            let n = chunk[..4 - pad]
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &c)| n | (val(c) << (18 - 6 * i)));
            out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
        }
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn data_url_round_trips_hostile_bodies() {
        for html in [
            "",
            "a",
            "ab",
            "abc",
            "<a href=\"context://open-link?url=%22%3E%3Cimg src=x onerror=alert(1)%3E\">x</a>",
            "100% <b>#fragment</b> %2523 %",
            "Привіт, світ — ünïcødé 🚚",
            "line\nbreak\ttab\r\n",
        ] {
            let url = data_url(html);
            let body = &url["data:text/html;charset=utf-8;base64,".len()..];
            // Nothing for Chromium to percent-decode, no fragment, no whitespace.
            assert!(
                body.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
                "{url}"
            );
            assert_eq!(decode_data_url(&url), html);
        }
    }

    #[test]
    fn every_page_is_a_base64_data_url() {
        let theme = THEMES[0];
        for url in [
            help::page_url(&theme),
            switcher::page_url(&theme, &[(1, "https://a.test/#x".into())]),
            omnibox::page_url(&theme, &["#1".into()], "https://e.test/?q="),
            passwords_list::unlock_url(&theme, true, Some("#err")),
        ] {
            assert!(url.starts_with("data:text/html;charset=utf-8;base64,"));
            assert!(!url.contains('#') && !url.contains('%'), "{url}");
            assert!(decode_data_url(&url).contains("</body>"));
        }
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
        assert!(html.contains("x&quot;&gt;&lt;script&gt;"));
        assert!(html.contains(">&lt;</span>"));
    }

    #[test]
    fn context_menu_link_cannot_inject_markup() {
        // Delivered by any web page via `context://hit?link=…`.
        let ctx = context_menu::MenuContext {
            link: Some("https://x.test/%22%3E%3Cimg src=x onerror=alert(1)%3E\"><b>".into()),
            ..Default::default()
        };
        let html = decode_data_url(&context_menu::page_url(&THEMES[0], &ctx));
        assert!(!html.contains("<img") && !html.contains("<b>"), "{html}");
        assert!(html.contains("context://open-link?url=https%3A%2F%2Fx.test%2F%2522%253E"));
    }

    #[test]
    fn passwords_list_escapes_web_controlled_fields() {
        use crate::persistence::vault::test_entry;
        let mut e = test_entry("id'); x('", "https://a.test\"><script>");
        e.username = "<img src=x onerror=1>".into();
        let never = vec!["https://n.test');alert(1);('".to_string()];
        let saved = decode_data_url(&passwords_list::page_url(
            &THEMES[0],
            &[e],
            &never,
            None,
            None,
            "saved",
        ));
        assert!(!saved.contains("<img") && !saved.contains("\"><script>"), "{saved}");
        assert!(saved.contains("a.test&quot;&gt;&lt;script&gt;"));
        assert!(saved.contains("password://go/fill?id=id%27%29%3B%20x%28%27"));
        let never_page = decode_data_url(&passwords_list::page_url(
            &THEMES[0], &[], &never, None, None, "never",
        ));
        assert!(!never_page.contains("');alert(1)"), "{never_page}");
        assert!(never_page.contains("remove-never?origin=https%3A%2F%2Fn.test%27%29%3Balert"));
    }
}
