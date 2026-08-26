// Injected autofill helper: detects login forms, asks Rust for vault
// matches via password://, fills fields, offers save after submit.
// Same fake-scheme pattern as clipboard_bridge.rs.

/// Re-injected on every top-level load of non-ephemeral pages.
pub const AUTOFILL_BRIDGE_SCRIPT: &str = r#"
(function() {
  if (window.__spatialAutofillBridge__) return;
  window.__spatialAutofillBridge__ = true;

  function originOf() {
    try { return location.origin; } catch (e) { return ''; }
  }

  function setNativeValue(el, value) {
    if (!el) return;
    var proto = el.tagName === 'TEXTAREA'
      ? window.HTMLTextAreaElement.prototype
      : window.HTMLInputElement.prototype;
    var desc = Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && desc.set) desc.set.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function scoreUsername(el) {
    var t = ((el.type || '') + ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.autocomplete || '') + ' ' + (el.placeholder || '')).toLowerCase();
    if (t.indexOf('pass') >= 0) return -1;
    if (el.type === 'email' || t.indexOf('email') >= 0 || t.indexOf('user') >= 0 || t.indexOf('login') >= 0) return 2;
    if (el.type === 'text' || !el.type) return 1;
    return 0;
  }

  function findFields(root) {
    root = root || document;
    var pwd = root.querySelector('input[type="password"]');
    if (!pwd) return null;
    var form = pwd.form || pwd.closest('form') || document;
    var inputs = Array.prototype.slice.call(form.querySelectorAll('input'));
    var user = null, best = 0;
    inputs.forEach(function(el) {
      if (el === pwd || el.type === 'hidden' || el.type === 'submit') return;
      var s = scoreUsername(el);
      if (s > best) { best = s; user = el; }
    });
    return { form: form, password: pwd, username: user };
  }

  function fieldByHints(hints) {
    var inputs = Array.prototype.slice.call(document.querySelectorAll('input, textarea'));
    for (var i = 0; i < inputs.length; i++) {
      var el = inputs[i];
      var blob = ((el.autocomplete || '') + ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.type || '')).toLowerCase();
      for (var h = 0; h < hints.length; h++) {
        if (blob.indexOf(hints[h]) >= 0) return el;
      }
    }
    return null;
  }

  window.__spatialAutofillFill = function(entry) {
    var fields = findFields();
    if (fields) {
      if (fields.username && entry.username) setNativeValue(fields.username, entry.username);
      if (fields.password && entry.password) setNativeValue(fields.password, entry.password);
    }
    if (entry.email) setNativeValue(fieldByHints(['email']), entry.email);
    if (entry.given_name) setNativeValue(fieldByHints(['given-name', 'firstname', 'first_name', 'fname']), entry.given_name);
    if (entry.family_name) setNativeValue(fieldByHints(['family-name', 'lastname', 'last_name', 'lname']), entry.family_name);
    if (entry.address_line1) setNativeValue(fieldByHints(['street-address', 'address-line1', 'address1']), entry.address_line1);
    if (entry.city) setNativeValue(fieldByHints(['address-level2', 'city']), entry.city);
    if (entry.postal_code) setNativeValue(fieldByHints(['postal-code', 'zip']), entry.postal_code);
    if (entry.country) setNativeValue(fieldByHints(['country']), entry.country);
    hidePicker();
  };

  function hidePicker() {
    var el = document.getElementById('__spatial_pw_picker');
    if (el) el.remove();
  }

  window.__spatialAutofillShowPicker = function(items) {
    hidePicker();
    if (!items || !items.length) return;
    var box = document.createElement('div');
    box.id = '__spatial_pw_picker';
    box.style.cssText = 'position:fixed;z-index:2147483647;right:16px;bottom:16px;max-width:320px;background:#1a1b26;color:#c0caf5;border:1px solid #565f89;border-radius:10px;padding:10px 12px;font:13px/1.4 system-ui,sans-serif;box-shadow:0 8px 24px rgba(0,0,0,.45)';
    var title = document.createElement('div');
    title.textContent = 'Saved logins';
    title.style.cssText = 'font-weight:600;margin-bottom:8px;opacity:.9';
    box.appendChild(title);
    items.forEach(function(it) {
      var btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = it.username || '(no username)';
      btn.style.cssText = 'display:block;width:100%;text-align:left;margin:4px 0;padding:8px 10px;border-radius:6px;border:1px solid #565f89;background:#24283b;color:#c0caf5;cursor:pointer';
      btn.onclick = function() {
        location = 'password://fill?id=' + encodeURIComponent(it.id);
      };
      box.appendChild(btn);
    });
    var dismiss = document.createElement('button');
    dismiss.type = 'button';
    dismiss.textContent = 'Dismiss';
    dismiss.style.cssText = 'margin-top:8px;background:transparent;border:none;color:#7aa2f7;cursor:pointer;padding:4px 0';
    dismiss.onclick = hidePicker;
    box.appendChild(dismiss);
    document.documentElement.appendChild(box);
  };

  function hideSave() {
    var el = document.getElementById('__spatial_pw_save');
    if (el) el.remove();
  }

  window.__spatialAutofillShowSave = function(payload) {
    hideSave();
    var box = document.createElement('div');
    box.id = '__spatial_pw_save';
    box.style.cssText = 'position:fixed;z-index:2147483647;left:16px;bottom:16px;max-width:360px;background:#1a1b26;color:#c0caf5;border:1px solid #565f89;border-radius:10px;padding:12px 14px;font:13px/1.4 system-ui,sans-serif;box-shadow:0 8px 24px rgba(0,0,0,.45)';
    box.innerHTML = '<div style="font-weight:600;margin-bottom:6px">Save password?</div>' +
      '<div style="opacity:.75;margin-bottom:10px;word-break:break-all">' +
      (payload.username || '') + ' @ ' + (payload.origin || '') + '</div>';
    function btn(label, href, primary) {
      var b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.style.cssText = 'margin:0 6px 0 0;padding:7px 12px;border-radius:6px;border:1px solid #565f89;cursor:pointer;' +
        (primary ? 'background:#7aa2f7;color:#1a1b26;font-weight:600' : 'background:#24283b;color:#c0caf5');
      b.onclick = function() {
        if (href) location = href;
        hideSave();
      };
      return b;
    }
    var q = 'origin=' + encodeURIComponent(payload.origin || '') +
      '&username=' + encodeURIComponent(payload.username || '') +
      '&password=' + encodeURIComponent(payload.password || '') +
      '&id=' + encodeURIComponent(payload.id || '');
    box.appendChild(btn('Save', 'password://save?' + q, true));
    box.appendChild(btn('Never for site', 'password://never?origin=' + encodeURIComponent(payload.origin || ''), false));
    box.appendChild(btn('Not now', '', false));
    document.documentElement.appendChild(box);
  };

  function requestQuery() {
    var o = originOf();
    if (!o || o === 'null' || o.indexOf('data:') === 0) return;
    location = 'password://query?origin=' + encodeURIComponent(o);
  }

  document.addEventListener('focusin', function(ev) {
    var t = ev.target;
    if (!t || !t.tagName || t.tagName !== 'INPUT') return;
    if (t.type === 'password' || scoreUsername(t) > 0) {
      if (!window.__spatialAutofillQueried) {
        window.__spatialAutofillQueried = true;
        setTimeout(requestQuery, 50);
      }
    }
  }, true);

  document.addEventListener('submit', function(ev) {
    var fields = findFields(ev.target);
    if (!fields || !fields.password || !fields.password.value) return;
    var o = originOf();
    if (!o) return;
    var user = (fields.username && fields.username.value) || '';
    location = 'password://save-offer?origin=' + encodeURIComponent(o) +
      '&username=' + encodeURIComponent(user) +
      '&password=' + encodeURIComponent(fields.password.value);
  }, true);
})();
"#;
