// Autofill: login forms ↔ password:// via hidden iframe (never assign
// location on the main frame — even canceled navigations blank the page).
// Picker/banner colors come from Theme.

use crate::output::Theme;

/// Build the bridge script for the current UI theme.
pub fn script(theme: &Theme) -> String {
    let radius = theme.css_radius();
    let radius_inner = theme.css_radius_inner();
    // Theme strings are always rgb(...); safe inside a JS single-quoted
    // cssText literal (no `#`, no quotes).
    format!(
        r#"
(function() {{
  // Re-inject after unlock: field may already be focused (no new focusin).
  if (window.__spatialAutofillBridge__) {{
    try {{ window.__spatialAutofillKick && window.__spatialAutofillKick(); }} catch (e) {{}}
    return;
  }}
  window.__spatialAutofillBridge__ = true;

  var UI = {{
    bg: '{bg}',
    fg: '{fg}',
    card: '{card}',
    border: '{border}',
    accent: '{accent}',
    accentFg: '{accent_fg}',
    radius: '{radius}',
    radiusInner: '{radius_inner}'
  }};

  function signal(url) {{
    try {{
      var ifr = document.createElement('iframe');
      ifr.setAttribute('aria-hidden', 'true');
      ifr.style.cssText = 'position:absolute;width:0;height:0;border:0;visibility:hidden';
      ifr.src = url;
      (document.documentElement || document.body).appendChild(ifr);
      setTimeout(function() {{ try {{ ifr.remove(); }} catch (e) {{}} }}, 2500);
    }} catch (e) {{}}
  }}

  function originOf() {{
    try {{ return location.origin || (location.protocol + '//' + location.host); }} catch (e) {{ return ''; }}
  }}

  function setNativeValue(el, value) {{
    if (!el) return;
    var proto = el.tagName === 'TEXTAREA'
      ? window.HTMLTextAreaElement.prototype
      : window.HTMLInputElement.prototype;
    var desc = Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && desc.set) desc.set.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  }}

  function scoreUsername(el) {{
    var t = ((el.type || '') + ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.autocomplete || '') + ' ' + (el.placeholder || '')).toLowerCase();
    if (t.indexOf('pass') >= 0) return -1;
    if (el.type === 'email' || t.indexOf('email') >= 0 || t.indexOf('user') >= 0 || t.indexOf('login') >= 0) return 2;
    if (el.type === 'text' || !el.type) return 1;
    return 0;
  }}

  function findFields(root) {{
    root = root || document;
    var pwd = root.querySelector('input[type="password"]');
    if (!pwd) return null;
    var form = pwd.form || pwd.closest('form') || document;
    var inputs = Array.prototype.slice.call(form.querySelectorAll('input'));
    var user = null, best = 0;
    inputs.forEach(function(el) {{
      if (el === pwd || el.type === 'hidden' || el.type === 'submit') return;
      var s = scoreUsername(el);
      if (s > best) {{ best = s; user = el; }}
    }});
    return {{ form: form, password: pwd, username: user }};
  }}

  function fieldByHints(hints) {{
    var inputs = Array.prototype.slice.call(document.querySelectorAll('input, textarea'));
    for (var i = 0; i < inputs.length; i++) {{
      var el = inputs[i];
      var blob = ((el.autocomplete || '') + ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.type || '')).toLowerCase();
      for (var h = 0; h < hints.length; h++) {{
        if (blob.indexOf(hints[h]) >= 0) return el;
      }}
    }}
    return null;
  }}

  window.__spatialAutofillFill = function(entry) {{
    var fields = findFields();
    if (fields) {{
      if (fields.username && entry.username) setNativeValue(fields.username, entry.username);
      if (fields.password && entry.password) setNativeValue(fields.password, entry.password);
    }}
    if (entry.email) setNativeValue(fieldByHints(['email']), entry.email);
    if (entry.address_line1) setNativeValue(fieldByHints(['street-address', 'address-line1', 'address1']), entry.address_line1);
    if (entry.city) setNativeValue(fieldByHints(['address-level2', 'city']), entry.city);
    if (entry.postal_code) setNativeValue(fieldByHints(['postal-code', 'zip']), entry.postal_code);
    if (entry.country) setNativeValue(fieldByHints(['country']), entry.country);
    hidePicker();
  }};

  function hidePicker() {{
    var el = document.getElementById('__spatial_pw_picker');
    if (el) el.remove();
  }}

  window.__spatialAutofillShowPicker = function(items) {{
    hidePicker();
    hideSave();
    if (!items || !items.length) return;
    var box = document.createElement('div');
    box.id = '__spatial_pw_picker';
    // Same corner as the save banner (left) — not opposite sides.
    box.style.cssText = 'position:fixed;z-index:2147483647;left:16px;bottom:16px;max-width:320px;background:'+UI.bg+';color:'+UI.fg+';border:1px solid '+UI.border+';border-radius:'+UI.radius+';padding:10px 12px;font:13px/1.4 system-ui,sans-serif;box-shadow:0 8px 24px rgba(0,0,0,.45)';
    var title = document.createElement('div');
    title.textContent = items.length === 1 ? 'Use saved login?' : 'Use saved login';
    title.style.cssText = 'font-weight:600;margin-bottom:8px;opacity:.9';
    box.appendChild(title);
    items.forEach(function(it) {{
      var btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = it.username || '(no username)';
      btn.style.cssText = 'display:block;width:100%;text-align:left;margin:4px 0;padding:8px 10px;border-radius:'+UI.radiusInner+';border:1px solid '+UI.border+';background:'+UI.card+';color:'+UI.fg+';cursor:pointer';
      btn.onclick = function(ev) {{
        ev.preventDefault();
        signal('password://go/fill?id=' + encodeURIComponent(it.id));
      }};
      box.appendChild(btn);
    }});
    var dismiss = document.createElement('button');
    dismiss.type = 'button';
    dismiss.textContent = 'Not now';
    dismiss.style.cssText = 'margin-top:8px;background:transparent;border:none;color:'+UI.accent+';cursor:pointer;padding:4px 0';
    dismiss.onclick = hidePicker;
    box.appendChild(dismiss);
    document.documentElement.appendChild(box);
  }};

  function hideSave() {{
    var el = document.getElementById('__spatial_pw_save');
    if (el) el.remove();
  }}

  window.__spatialAutofillShowSave = function(payload) {{
    hideSave();
    hidePicker();
    var box = document.createElement('div');
    box.id = '__spatial_pw_save';
    box.style.cssText = 'position:fixed;z-index:2147483647;left:16px;bottom:16px;max-width:360px;background:'+UI.bg+';color:'+UI.fg+';border:1px solid '+UI.border+';border-radius:'+UI.radius+';padding:12px 14px;font:13px/1.4 system-ui,sans-serif;box-shadow:0 8px 24px rgba(0,0,0,.45)';
    var head = document.createElement('div');
    head.style.cssText = 'font-weight:600;margin-bottom:6px';
    head.textContent = 'Save password?';
    var sub = document.createElement('div');
    sub.style.cssText = 'opacity:.75;margin-bottom:10px;word-break:break-all';
    sub.textContent = (payload.username || '') + ' @ ' + (payload.origin || '');
    box.appendChild(head);
    box.appendChild(sub);
    function btn(label, href, primary) {{
      var b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.style.cssText = 'margin:0 6px 0 0;padding:7px 12px;border-radius:'+UI.radiusInner+';border:1px solid '+UI.border+';cursor:pointer;' +
        (primary ? 'background:'+UI.accent+';color:'+UI.accentFg+';font-weight:600' : 'background:'+UI.card+';color:'+UI.fg);
      b.onclick = function(ev) {{
        ev.preventDefault();
        if (href) signal(href);
        hideSave();
      }};
      return b;
    }}
    var q = 'origin=' + encodeURIComponent(payload.origin || '') +
      '&username=' + encodeURIComponent(payload.username || '') +
      '&password=' + encodeURIComponent(payload.password || '') +
      '&id=' + encodeURIComponent(payload.id || '');
    box.appendChild(btn('Save', 'password://go/save?' + q, true));
    box.appendChild(btn('Never for site', 'password://go/never?origin=' + encodeURIComponent(payload.origin || ''), false));
    box.appendChild(btn('Not now', '', false));
    document.documentElement.appendChild(box);
  }};

  function requestQuery() {{
    var o = originOf();
    if (!o || o === 'null' || o.indexOf('data:') === 0) return;
    // Path-style host (`go`) avoids CEF turning `password://query?…` into
    // `password://query/?…` and breaking parse_password_action.
    signal('password://go/query?origin=' + encodeURIComponent(o));
  }}

  // Re-query on each login-field focus (debounced). The old once-per-page
  // flag meant: first focus while vault locked → never suggest again;
  // context-menu Fill was the only path that still worked.
  var lastQueryAt = 0;
  function maybeQueryFromFocus(t) {{
    if (!t || !t.tagName || t.tagName !== 'INPUT') return;
    if (t.type === 'password' || scoreUsername(t) > 0) {{
      var now = Date.now();
      if (now - lastQueryAt < 400) return;
      lastQueryAt = now;
      setTimeout(requestQuery, 50);
    }}
  }}
  window.__spatialAutofillKick = function() {{
    lastQueryAt = 0;
    setTimeout(requestQuery, 30);
  }};
  document.addEventListener('focusin', function(ev) {{
    maybeQueryFromFocus(ev.target);
  }}, true);
  // Some sites move focus before our bridge loads; also catch click into fields.
  document.addEventListener('pointerdown', function(ev) {{
    maybeQueryFromFocus(ev.target);
  }}, true);

  document.addEventListener('submit', function(ev) {{
    var fields = findFields(ev.target);
    if (!fields || !fields.password || !fields.password.value) return;
    var o = originOf();
    if (!o) return;
    var user = (fields.username && fields.username.value) || '';
    var pass = fields.password.value;
    // Only signal Rust — do NOT show a local fallback banner. Rust skips
    // identical username+password; a JS fallback ignored that and always
    // popped "Save password?" after autofill + submit.
    signal('password://go/save-offer?origin=' + encodeURIComponent(o) +
      '&username=' + encodeURIComponent(user) +
      '&password=' + encodeURIComponent(pass));
  }}, true);

  // Right-click hit-test: compositor calls this with DIP coords, then
  // opens an in-canvas menu from the context://hit signal.
  window.__spatialContextHitAt = function(x, y) {{
    var el = document.elementFromPoint(x, y);
    var link = '', image = '', pwd = false;
    var n = el;
    while (n && n.nodeType === 1) {{
      if (!link && n.tagName === 'A' && n.href) link = n.href;
      if (!image && n.tagName === 'IMG' && n.src) image = n.src;
      if (n.tagName === 'INPUT' && (n.type || '').toLowerCase() === 'password') pwd = true;
      n = n.parentElement;
    }}
    signal('context://hit?link=' + encodeURIComponent(link) +
      '&image=' + encodeURIComponent(image) +
      '&pwd=' + (pwd ? '1' : '0') +
      '&page=' + encodeURIComponent(location.href));
  }};

  // Suppress Chromium's own context menu — we draw ours on the canvas.
  document.addEventListener('contextmenu', function(ev) {{
    ev.preventDefault();
  }}, true);
}})();
"#,
        bg = theme.help_bg,
        fg = theme.help_fg,
        card = theme.help_card_bg,
        border = theme.help_card_border,
        accent = theme.help_key_bg,
        accent_fg = theme.help_key_fg,
        radius = radius,
        radius_inner = radius_inner,
    )
}
