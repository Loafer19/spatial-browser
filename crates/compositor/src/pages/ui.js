// spatial-browser built-in pages: shared keyboard model, list filter, live
// data and theme hooks. Inlined into every page by `ui::document`.
//
// Rows: `.row[data-open]` (Enter / click opens that URL) or `.row[data-nav]`
// (selectable only). `data-delete` on a row: Delete removes it (with a
// second Delete when `data-confirm` is set — no window.confirm in OSR).
// `input[data-filter]` filters rows client-side; `body[data-escape]` is
// where Escape goes when nothing else wants it.
// Rust pushes data with `ui.push(name, value)`; pages subscribe with
// `ui.on(name, fn)` (pushes before subscription are kept and replayed).
(function () {
  'use strict';
  var active = -1;
  var handlers = {};
  var queued = {};

  function rows() {
    return Array.prototype.filter.call(
      document.querySelectorAll('.row[data-open], .row[data-nav]'),
      function (r) { return r.offsetParent !== null; }
    );
  }

  function setActive(index, scroll) {
    var list = rows();
    document.querySelectorAll('.row.is-active').forEach(function (r) { r.classList.remove('is-active'); });
    if (!list.length) { active = -1; return; }
    active = ((index % list.length) + list.length) % list.length;
    list[active].classList.add('is-active');
    if (scroll !== false) list[active].scrollIntoView({ block: 'nearest' });
  }

  function activeRow() { return rows()[active] || null; }

  function go(url) { if (url) window.location = url; }

  function openRow(row) {
    if (!row) return;
    if (row.dataset.open) go(row.dataset.open);
    else if (row.tagName === 'A' && row.href) go(row.href);
  }

  // OSR tiles have no JS dialog handler (window.confirm never shows), so a
  // row with `data-confirm` needs a second Delete within 2.5 s.
  var armed = null;
  var armedTimer = 0;
  function deleteRow(row) {
    if (!row || !row.dataset.delete) return;
    if (row.dataset.confirm && armed !== row) {
      armed = row;
      clearTimeout(armedTimer);
      armedTimer = setTimeout(function () { armed = null; }, 2500);
      window.ui.toast(row.dataset.confirm + ' \u2014 press Delete again');
      return;
    }
    armed = null;
    go(row.dataset.delete);
  }

  function isTyping(el) {
    return el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  }

  function searchBox() {
    return document.querySelector('input[data-filter]') || document.querySelector('.search input, input[type=search]');
  }

  // ---- client-side filter --------------------------------------------------
  function applyFilter(input) {
    var terms = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    var scope = input.dataset.filter ? document.querySelector(input.dataset.filter) || document : document;
    var shown = 0;
    scope.querySelectorAll('.row').forEach(function (row) {
      var text = (row.dataset.text || row.textContent).toLowerCase();
      var hit = terms.every(function (t) { return text.indexOf(t) >= 0; });
      row.classList.toggle('hidden', !hit);
      if (hit) shown++;
    });
    scope.querySelectorAll('[data-group]').forEach(function (group) {
      group.classList.toggle('hidden', !group.querySelector('.row:not(.hidden)'));
    });
    document.querySelectorAll('[data-filter-empty]').forEach(function (el) {
      el.classList.toggle('hidden', shown > 0);
    });
    document.querySelectorAll('[data-filter-count]').forEach(function (el) { el.textContent = String(shown); });
    setActive(0, true);
  }

  document.addEventListener('input', function (event) {
    if (event.target.matches && event.target.matches('input[data-filter]')) applyFilter(event.target);
  });

  // ---- keyboard ------------------------------------------------------------
  function overlay() { return document.querySelector('.ui-overlay'); }

  function toggleKeys() {
    var open = overlay();
    if (open) { open.remove(); return; }
    var tpl = document.getElementById('ui-keys');
    if (!tpl) return;
    var wrap = document.createElement('div');
    wrap.className = 'ui-overlay';
    wrap.appendChild(tpl.content.cloneNode(true));
    wrap.addEventListener('click', function (e) { if (e.target === wrap) wrap.remove(); });
    document.body.appendChild(wrap);
  }

  document.addEventListener('keydown', function (event) {
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
    var target = event.target;
    var key = event.key;

    if (key === 'Escape') {
      if (overlay()) { overlay().remove(); event.preventDefault(); return; }
      if (isTyping(target)) {
        if (target.matches('input[data-filter]') && target.value) {
          target.value = '';
          applyFilter(target);
        } else {
          target.blur();
        }
        event.preventDefault();
        return;
      }
      if (document.body.dataset.escape) { event.preventDefault(); go(document.body.dataset.escape); }
      return;
    }

    if (isTyping(target)) {
      // A filter box drives the list without losing focus.
      if (!target.matches('input[data-filter]')) return;
      if (key === 'ArrowDown') { event.preventDefault(); setActive(active + 1); }
      else if (key === 'ArrowUp') { event.preventDefault(); setActive(active - 1); }
      else if (key === 'Enter') { event.preventDefault(); openRow(activeRow()); }
      return;
    }

    switch (key) {
      case 'ArrowDown': case 'j': event.preventDefault(); setActive(active + 1); break;
      case 'ArrowUp': case 'k': event.preventDefault(); setActive(active - 1); break;
      case 'Home': case 'g': event.preventDefault(); setActive(0); break;
      case 'End': case 'G': event.preventDefault(); setActive(rows().length - 1); break;
      case 'Enter': {
        // A focused control (button, link) handles its own Enter.
        if (target.matches && target.matches('a, button, summary, [tabindex]:not(.row)')) return;
        event.preventDefault();
        openRow(activeRow());
        break;
      }
      case 'Delete': event.preventDefault(); deleteRow(activeRow()); break;
      case '/': {
        var box = searchBox();
        if (box) { event.preventDefault(); box.focus(); box.select(); }
        break;
      }
      case '?': event.preventDefault(); toggleKeys(); break;
    }
  });

  document.addEventListener('mousemove', function (event) {
    var row = event.target.closest && event.target.closest('.row[data-open], .row[data-nav]');
    if (!row) return;
    var index = rows().indexOf(row);
    if (index >= 0 && index !== active) setActive(index, false);
  });

  document.addEventListener('click', function (event) {
    var row = event.target.closest && event.target.closest('.row[data-open]');
    // Controls inside a row (buttons, links, inputs) do their own thing.
    if (!row || event.target.closest('a, button, input, select, textarea, label, form')) return;
    openRow(row);
  });

  window.addEventListener('load', function () {
    var box = document.querySelector('input[data-filter]');
    if (box && box.value) applyFilter(box);
    else if (!document.body.hasAttribute('data-no-row')) setActive(0, false);
    var auto = document.querySelector('[autofocus]');
    if (auto) auto.focus();
  });

  // ---- public hooks --------------------------------------------------------
  var toastTimer = 0;
  window.ui = {
    rows: rows,
    active: activeRow,
    setActive: setActive,
    go: go,
    on: function (name, fn) {
      handlers[name] = fn;
      if (name in queued) { var v = queued[name]; delete queued[name]; fn(v); }
    },
    push: function (name, value) {
      if (handlers[name]) handlers[name](value);
      else queued[name] = value;
    },
    theme: function (vars) {
      var style = document.documentElement.style;
      Object.keys(vars).forEach(function (k) { style.setProperty(k, vars[k]); });
      if (vars['color-scheme']) style.colorScheme = vars['color-scheme'];
    },
    toast: function (text) {
      var el = document.querySelector('.ui-toast');
      if (!el) { el = document.createElement('div'); el.className = 'ui-toast'; document.body.appendChild(el); }
      el.textContent = text;
      el.classList.add('show');
      clearTimeout(toastTimer);
      toastTimer = setTimeout(function () { el.classList.remove('show'); }, 1800);
    },
    filter: applyFilter,
  };
})();
