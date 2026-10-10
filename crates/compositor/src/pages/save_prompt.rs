// "Save password?" prompt: a small ephemeral tile above the page that
// offered a login, owned by the compositor. Unlike a banner in the page's
// own DOM, the page can't press Save itself or hide the prompt, and it
// survives the post-login navigation. The password never reaches this HTML.

use super::{data_url, html_escape};
use crate::output::Theme;

/// `is_update`: the login would overwrite a saved entry's password, which
/// the prompt says plainly so an unexpected offer stands out.
pub fn page_url(theme: &Theme, host: &str, username: &str, is_update: bool) -> String {
    let button = |primary: bool| {
        format!(
            "padding:6px 12px;border-radius:6px;border:1px solid {border};cursor:pointer;font:inherit;{look}",
            border = theme.help_card_border,
            look = if primary {
                format!("background:{};color:{};font-weight:600", theme.help_key_bg, theme.help_key_fg)
            } else {
                format!("background:{};color:{}", theme.help_card_bg, theme.help_fg)
            },
        )
    };
    let who = if username.is_empty() {
        String::new()
    } else {
        format!("{} · ", html_escape(username))
    };
    data_url(&format!(
        "<body style=\"margin:0;padding:8px 10px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:13px;display:flex;gap:8px;\
         align-items:center;box-sizing:border-box;height:100vh\">\
         <span style=\"flex:1;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis\">\
         <b>{title}</b> <span style=\"opacity:0.75\">{who}{host}</span></span>\
         <button autofocus onclick=\"location='password://go/prompt-save'\" style=\"{primary}\">Save</button>\
         <button onclick=\"location='password://go/prompt-never'\" style=\"{plain}\">Never</button>\
         <button onclick=\"location='password://go/prompt-dismiss'\" style=\"{plain}\">Not now</button>\
         </body>",
        bg = theme.help_bg,
        fg = theme.help_fg,
        host = html_escape(host),
        title = if is_update { "Update saved password?" } else { "Save password?" },
        primary = button(true),
        plain = button(false),
    ))
}

#[cfg(test)]
mod tests {
    use crate::output::THEMES;
    use crate::pages::tests::decode_data_url;

    #[test]
    fn escapes_host_and_username() {
        let html = decode_data_url(&super::page_url(&THEMES[0], "a.test", "<img src=x onerror=alert(1)>", false));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img"));
    }
}
