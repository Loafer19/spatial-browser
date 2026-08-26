// Ctrl+Shift+U: userscripts + userstyles list. Scripts and styles share
// this page; `userscripts://reload` reloads both from disk.

use super::{html_escape, LIST_NAV_SCRIPT};
use crate::output::Theme;
use crate::userscripts::{RunAt, UserScript};
use crate::userstyles::UserStyle;

pub(crate) fn page_url(theme: &Theme, scripts: &[UserScript], styles: &[UserStyle]) -> String {
    let radius = theme.css_radius();
    let radius_inner = theme.css_radius_inner();
    let mut script_rows = String::new();
    if scripts.is_empty() {
        script_rows.push_str(
            "<p style=\"opacity:0.7;margin:8px 0\">No scripts yet. Drop a <code>.js</code> with \
             <code>// @match</code> (use <code>spatial-ui</code> for built-in lists) into \
             the userscripts folder, then Reload.</p>",
        );
    }
    for script in scripts {
        let state = if script.enabled { "On" } else { "Off" };
        let run = match script.run_at {
            RunAt::DocumentStart => "document-start",
            RunAt::DocumentEnd => "document-end",
            RunAt::DocumentIdle => "document-idle",
        };
        let matches = html_escape(&script.matches.join(", "));
        let name = html_escape(&script.name);
        let file = html_escape(&script.file_name);
        let toggle = format!("userscripts://toggle/{}", urlencoding_path(&script.file_name));
        script_rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"{toggle}\" onclick=\"location='{toggle}'\" \
             style=\"display:flex;flex-direction:column;gap:4px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:{radius};\
             border:1px solid {card_border};opacity:{opacity}\">\
             <div style=\"display:flex;align-items:center;gap:10px\">\
             <span style=\"flex:1;font-weight:600;color:{fg}\">{name}</span>\
             <span style=\"flex-shrink:0;color:{fg};opacity:0.7\">{state}</span>\
             </div>\
             <div style=\"font-size:12px;color:{fg};opacity:0.65\">{file} · {run}</div>\
             <div style=\"font-size:12px;color:{fg};opacity:0.55;word-break:break-all\">{matches}</div>\
             </div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_heading,
            opacity = if script.enabled { "1" } else { "0.55" },
        ));
    }

    let mut style_rows = String::new();
    if styles.is_empty() {
        style_rows.push_str(
            "<p style=\"opacity:0.7;margin:8px 0\">No styles yet. Drop a <code>.css</code> with \
             <code>/* @match */</code> (use <code>spatial-ui</code> for built-in lists) into \
             the userstyles folder, then Reload.</p>",
        );
    }
    for style in styles {
        let state = if style.enabled { "On" } else { "Off" };
        let matches = html_escape(&style.matches.join(", "));
        let name = html_escape(&style.name);
        let file = html_escape(&style.file_name);
        let toggle = format!(
            "userscripts://toggle-style/{}",
            urlencoding_path(&style.file_name)
        );
        style_rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"{toggle}\" onclick=\"location='{toggle}'\" \
             style=\"display:flex;flex-direction:column;gap:4px;padding:10px 14px;\
             cursor:pointer;background:{card_bg};border-radius:{radius};\
             border:1px solid {card_border};opacity:{opacity}\">\
             <div style=\"display:flex;align-items:center;gap:10px\">\
             <span style=\"flex:1;font-weight:600;color:{fg}\">{name}</span>\
             <span style=\"flex-shrink:0;color:{fg};opacity:0.7\">{state}</span>\
             </div>\
             <div style=\"font-size:12px;color:{fg};opacity:0.65\">{file}</div>\
             <div style=\"font-size:12px;color:{fg};opacity:0.55;word-break:break-all\">{matches}</div>\
             </div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_heading,
            opacity = if style.enabled { "1" } else { "0.55" },
        ));
    }

    let actions = format!(
        "<div style=\"display:flex;gap:8px;margin:0 0 12px;flex-wrap:wrap\">\
         <button onclick=\"location='userscripts://reload'\" style=\"\
         background:{key_bg};color:{key_fg};border:none;border-radius:{radius_inner};\
         padding:8px 12px;cursor:pointer;font-weight:600\">Reload</button>\
         <button onclick=\"location='userscripts://open-dir'\" style=\"\
         background:{card_bg};color:{fg};border:1px solid {card_border};border-radius:{radius_inner};\
         padding:8px 12px;cursor:pointer\">Scripts folder</button>\
         <button onclick=\"location='userscripts://open-styles-dir'\" style=\"\
         background:{card_bg};color:{fg};border:1px solid {card_border};border-radius:{radius_inner};\
         padding:8px 12px;cursor:pointer\">Styles folder</button>\
         </div>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        card_bg = theme.help_card_bg,
        card_border = theme.help_card_border,
        fg = theme.help_heading,
    );

    format!(
        "data:text/html;charset=utf-8,{body_open}\
         <h1 style=\"margin:0 0 4px;color:{heading};font-size:20px\">Scripts &amp; styles</h1>\
         <p style=\"margin:0 0 12px;opacity:0.7;font-size:13px\">\
         Both honor <code>spatial-ui</code> for built-in pages · \
         Scripts: <code>// @match</code> · Styles: <code>/* @match */</code></p>\
         {actions}\
         <h2 style=\"margin:8px 0;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Userscripts</h2>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{script_rows}</div>\
         <h2 style=\"margin:20px 0 8px;font-size:13px;text-transform:uppercase;\
         letter-spacing:0.05em;color:{heading};opacity:0.8\">Userstyles</h2>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{style_rows}</div>\
         {LIST_NAV_SCRIPT}</body>",
        body_open = super::body_open(theme, ""),
        heading = theme.help_heading,
    )
}

fn urlencoding_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
