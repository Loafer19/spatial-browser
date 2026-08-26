// Ctrl+Shift+P passwords UI: unlock/create vault, list/edit entries,
// generator, never-save list. Actions via `password://...`.

use super::{html_escape, LIST_NAV_SCRIPT};
use crate::output::Theme;
use crate::persistence::vault::VaultEntry;

pub(crate) fn unlock_url(theme: &Theme, create: bool, error: Option<&str>) -> String {
    let heading = if create {
        "Create password vault"
    } else {
        "Unlock password vault"
    };
    let err = error
        .map(|e| {
            format!(
                "<p style=\"color:#f7768e;margin:0 0 12px\">{}</p>",
                html_escape(e)
            )
        })
        .unwrap_or_default();
    let confirm = if create {
        format!(
            "<label style=\"display:block;margin:10px 0 4px;opacity:0.8\">Confirm</label>\
             <input name=\"confirm\" type=\"password\" required autocomplete=\"new-password\" \
             style=\"width:100%;box-sizing:border-box;padding:10px 12px;border-radius:8px;\
             border:1px solid {border};background:{card};color:{fg};font-size:15px\">",
            border = theme.help_card_border,
            card = theme.help_card_bg,
            fg = theme.help_heading,
        )
    } else {
        String::new()
    };
    let create_flag = if create { "1" } else { "0" };
    format!(
        "data:text/html;charset=utf-8,{body}\
         <h1 style=\"margin:0 0 8px;color:{heading_c};font-size:20px\">{heading}</h1>\
         <p style=\"opacity:0.7;margin:0 0 12px;font-size:13px\">\
         Local encrypted vault (AES-GCM). Unlocks for this session only.</p>\
         {err}\
         <form onsubmit=\"event.preventDefault();\
           var p=this.password.value;\
           var c=this.confirm?this.confirm.value:'';\
           location='password://unlock?create={create_flag}&password='+encodeURIComponent(p)+\
           (c?'&confirm='+encodeURIComponent(c):'');\">\
         <label style=\"display:block;margin:0 0 4px;opacity:0.8\">Master password</label>\
         <input name=\"password\" type=\"password\" required autofocus autocomplete=\"current-password\" \
         style=\"width:100%;box-sizing:border-box;padding:10px 12px;border-radius:8px;\
         border:1px solid {border};background:{card};color:{fg};font-size:15px\">\
         {confirm}\
         <button type=\"submit\" style=\"margin-top:14px;padding:10px 16px;border:none;\
         border-radius:8px;background:{accent};color:{key_fg};font-weight:600;cursor:pointer\">\
         {btn}</button></form></body>",
        body = super::body_open(theme, ""),
        heading_c = theme.help_heading,
        border = theme.help_card_border,
        card = theme.help_card_bg,
        fg = theme.help_heading,
        accent = theme.help_key_bg,
        key_fg = theme.help_key_fg,
        btn = if create { "Create vault" } else { "Unlock" },
    )
}

pub(crate) fn page_url(
    theme: &Theme,
    entries: &[VaultEntry],
    never_save: &[String],
    generated: Option<&str>,
) -> String {
    let mut rows = String::new();
    if entries.is_empty() {
        rows.push_str(
            "<p style=\"opacity:0.7\">No saved logins yet. They appear after you accept a save \
             prompt on a site, or add one below.</p>",
        );
    }
    for e in entries {
        let origin = html_escape(&e.origin);
        let user = html_escape(&e.username);
        let id = html_escape(&e.id);
        rows.push_str(&format!(
            "<div class=\"list-row\" data-open=\"password://fill?id={id}\" \
             style=\"padding:10px 14px;background:{card};border:1px solid {border};\
             border-radius:8px;display:flex;flex-direction:column;gap:4px\">\
             <div style=\"display:flex;gap:8px;align-items:center\">\
             <span style=\"flex:1;font-weight:600;color:{fg}\">{user}</span>\
             <button type=\"button\" onclick=\"location='password://fill?id={id}'\" \
             style=\"padding:4px 10px;border-radius:6px;border:1px solid {border};\
             background:{accent};color:{key_fg};cursor:pointer;font-size:12px\">Fill</button>\
             <button type=\"button\" onclick=\"location='password://delete?id={id}'\" \
             style=\"padding:4px 10px;border-radius:6px;border:1px solid {border};\
             background:transparent;color:{fg};cursor:pointer;font-size:12px\">Delete</button>\
             </div>\
             <div style=\"font-size:12px;opacity:0.65;color:{fg};word-break:break-all\">{origin}</div>\
             </div>",
            card = theme.help_card_bg,
            border = theme.help_card_border,
            fg = theme.help_heading,
            accent = theme.help_key_bg,
            key_fg = theme.help_key_fg,
        ));
    }

    let mut never_rows = String::new();
    if never_save.is_empty() {
        never_rows.push_str("<p style=\"opacity:0.65;font-size:13px\">None</p>");
    }
    for o in never_save {
        let esc = html_escape(o);
        never_rows.push_str(&format!(
            "<div style=\"display:flex;gap:8px;align-items:center;padding:6px 0\">\
             <span style=\"flex:1;font-size:13px;word-break:break-all\">{esc}</span>\
             <button type=\"button\" onclick=\"location='password://remove-never?origin={esc}'\" \
             style=\"font-size:12px;cursor:pointer\">Remove</button></div>"
        ));
    }

    let gen_out = generated
        .map(|g| {
            format!(
                "<p style=\"margin:8px 0;padding:8px 10px;background:{card};border-radius:6px;\
                 font-family:monospace;word-break:break-all\">{g}</p>\
                 <button type=\"button\" onclick=\"location='clipboard://copy?text={enc}'\" \
                 style=\"padding:6px 12px;border-radius:6px;border:none;background:{accent};\
                 color:{key_fg};cursor:pointer\">Copy</button>",
                g = html_escape(g),
                enc = urlencoding_minimal(g),
                card = theme.help_card_bg,
                accent = theme.help_key_bg,
                key_fg = theme.help_key_fg,
            )
        })
        .unwrap_or_default();

    format!(
        "data:text/html;charset=utf-8,{body}\
         <h1 style=\"margin:0 0 8px;color:{heading};font-size:20px\">Passwords</h1>\
         <p style=\"opacity:0.7;font-size:13px;margin:0 0 14px\">Local vault · Fill active page, or save from site prompts</p>\
         <h2 style=\"font-size:13px;text-transform:uppercase;opacity:0.8;color:{heading}\">Saved</h2>\
         <div style=\"display:flex;flex-direction:column;gap:8px;margin-bottom:18px\">{rows}</div>\
         <h2 style=\"font-size:13px;text-transform:uppercase;opacity:0.8;color:{heading}\">Add entry</h2>\
         <form onsubmit=\"event.preventDefault();location='password://upsert?origin='+\
           encodeURIComponent(this.origin.value)+'&username='+encodeURIComponent(this.username.value)+\
           '&password='+encodeURIComponent(this.password.value)+\
           '&email='+encodeURIComponent(this.email.value)+\
           '&given_name='+encodeURIComponent(this.given_name.value)+\
           '&family_name='+encodeURIComponent(this.family_name.value);\" \
           style=\"display:flex;flex-direction:column;gap:8px;margin-bottom:18px\">\
         <input name=\"origin\" required placeholder=\"https://example.com\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <input name=\"username\" placeholder=\"Username\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <input name=\"password\" type=\"password\" required placeholder=\"Password\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <input name=\"email\" placeholder=\"Email (optional)\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <input name=\"given_name\" placeholder=\"Given name (optional)\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <input name=\"family_name\" placeholder=\"Family name (optional)\" \
           style=\"padding:8px 10px;border-radius:6px;border:1px solid {border};background:{card};color:{fg}\">\
         <button type=\"submit\" style=\"align-self:flex-start;padding:8px 14px;border:none;\
           border-radius:6px;background:{accent};color:{key_fg};font-weight:600;cursor:pointer\">Save</button>\
         </form>\
         <h2 style=\"font-size:13px;text-transform:uppercase;opacity:0.8;color:{heading}\">Generator</h2>\
         <div style=\"margin-bottom:18px\">\
         <button type=\"button\" onclick=\"location='password://generate?length=20&symbols=1'\" \
           style=\"padding:8px 14px;border:none;border-radius:6px;background:{accent};color:{key_fg};\
           font-weight:600;cursor:pointer\">Generate</button>\
         {gen_out}</div>\
         <h2 style=\"font-size:13px;text-transform:uppercase;opacity:0.8;color:{heading}\">Never save</h2>\
         {never_rows}\
         {LIST_NAV_SCRIPT}</body>",
        body = super::body_open(theme, ""),
        heading = theme.help_heading,
        border = theme.help_card_border,
        card = theme.help_card_bg,
        fg = theme.help_heading,
        accent = theme.help_key_bg,
        key_fg = theme.help_key_fg,
    )
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
