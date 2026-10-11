
// Ctrl+K palette page: ranks the server-rendered rows (PaletteRank), shows
// the best per section, best section first, and drives selection from the
// input. Rows only move, hide and get <mark>s here; their text came escaped
// from Rust and is only ever set back with textContent.
(function () {
  'use strict';
  var data = JSON.parse(document.getElementById('pal-data').textContent);
  var input = document.getElementById('q');
  var scope = document.getElementById('pal-scope');
  var results = document.getElementById('results');
  var empty = document.getElementById('pal-empty');
  var web = document.querySelector('section[data-section="web"]');
  var webGo = web.querySelector('[data-web="go"]');
  var webSearch = web.querySelector('[data-web="search"]');

  // Section bonuses: on a tie an open page beats a command beats a
  // bookmark beats history.
  var BONUS = { pages: 8, commands: 6, bookmarks: 4, history: 0 };
  var LIMIT = { pages: 5, commands: 6, bookmarks: 5, history: 8 };
  // A substring hit scores at least 50 (PaletteRank); with one present,
  // rows under KEEP × the best score are dropped.
  var STRONG = 50;
  var KEEP = 0.3;

  var sections = Array.prototype.map.call(
    document.querySelectorAll('section[data-section]:not([data-section="web"])'),
    function (el) {
      var kind = el.dataset.section;
      var items = Array.prototype.map.call(el.querySelectorAll('.row'), function (row, order) {
        return {
          el: row,
          order: order,
          t: row.dataset.t || '',
          u: row.dataset.u || '',
          k: row.dataset.k || '',
          r: row.dataset.r ? Number(row.dataset.r) : -1,
          at: row.dataset.at ? Number(row.dataset.at) : 0,
          featured: row.hasAttribute('data-featured'),
          title: row.querySelector('.title'),
          sub: row.querySelector('.sub'),
          meta: row.querySelector('.meta'),
          icon: row.querySelector('.favicon[data-host]'),
        };
      });
      return { el: el, kind: kind, list: el.querySelector('.pal-rows'), items: items };
    }
  );

  // Text with the characters at `hits` wrapped in <mark>.
  function paint(el, text, hits) {
    if (!el) return;
    el.textContent = '';
    if (!hits || !hits.length) { el.textContent = text; return; }
    var set = {};
    hits.forEach(function (i) { set[i] = true; });
    var buf = '';
    var marked = false;
    function flush() {
      if (!buf) return;
      if (marked) {
        var m = document.createElement('mark');
        m.textContent = buf;
        el.appendChild(m);
      } else {
        el.appendChild(document.createTextNode(buf));
      }
      buf = '';
    }
    for (var i = 0; i < text.length; i++) {
      var on = !!set[i];
      if (on !== marked) { flush(); marked = on; }
      buf += text.charAt(i);
    }
    flush();
  }

  function ago(at) {
    var s = Math.max(0, Date.now() / 1000 - at);
    if (s < 60) return 'now';
    if (s < 3600) return Math.floor(s / 60) + 'm';
    if (s < 86400) return Math.floor(s / 3600) + 'h';
    if (s < 86400 * 30) return Math.floor(s / 86400) + 'd';
    return new Date(at * 1000).toLocaleDateString();
  }

  // The cached favicon, attached the first time a row is shown.
  function attachIcon(item) {
    var slot = item.icon;
    if (!slot || slot.dataset.done) return;
    slot.dataset.done = '1';
    var uri = data.icons[slot.dataset.host];
    if (!uri) return;
    var img = document.createElement('img');
    img.alt = '';
    img.src = uri;
    slot.appendChild(img);
  }

  function showWeb(row, title, sub, url, raw) {
    row.querySelector('.title').textContent = title;
    row.querySelector('.sub').textContent = sub;
    row.dataset.open = 'omnibox://go?q=' + encodeURIComponent(raw) + '&url=' + encodeURIComponent(url);
    row.classList.remove('hidden');
  }

  function engineHost() {
    var m = /^[a-z]+:\/\/([^/?#]+)/i.exec(data.engine || '');
    return m ? m[1].replace(/^www\./, '') : 'the web';
  }

  function render() {
    var q = PaletteRank.parse(input.value);
    var words = PaletteRank.words(q.text);
    scope.textContent = q.scope === 'commands' ? 'Commands' : q.scope === 'pages' ? 'Open pages' : '';
    scope.classList.toggle('hidden', !q.scope);

    var groups = sections.map(function (sec) {
      var picked = [];
      if (!q.scope || q.scope === sec.kind) {
        if (!words.length) {
          // Empty query: open pages and the featured commands (all of a
          // prefixed section).
          picked = sec.items
            .filter(function (it) {
              if (q.scope) return true;
              if (sec.kind === 'pages') return true;
              return sec.kind === 'commands' && it.featured;
            })
            .map(function (it) { return { it: it, score: 0, hits: null }; });
        } else {
          sec.items.forEach(function (it) {
            var m = PaletteRank.score(it, words);
            if (m) picked.push({ it: it, score: m.score + BONUS[sec.kind], hits: m.hits });
          });
          picked.sort(function (a, b) { return b.score - a.score || a.it.order - b.it.order; });
          if (!q.scope) picked = picked.slice(0, LIMIT[sec.kind]);
        }
      }
      return { sec: sec, picked: picked, best: picked.length ? picked[0].score : -1 };
    });
    if (words.length) {
      // Once something matches as a substring, scattered fuzzy hits are
      // noise: keep only rows within reach of the best one.
      var top = Math.max.apply(null, groups.map(function (g) { return g.best; }));
      var floor = top >= STRONG ? top * KEEP : 0;
      groups.forEach(function (g) {
        g.picked = g.picked.filter(function (p) { return p.score >= floor; });
        g.best = g.picked.length ? g.picked[0].score : -1;
      });
      if (!q.scope) groups.sort(function (a, b) { return b.best - a.best; });
    }

    var shown = 0;
    groups.forEach(function (g) {
      results.insertBefore(g.sec.el, empty);
      g.sec.items.forEach(function (it) { it.el.classList.add('hidden'); });
      g.picked.forEach(function (p) {
        var it = p.it;
        paint(it.title, it.t, p.hits && p.hits[0]);
        paint(it.sub, it.u, p.hits && p.hits[1]);
        if (it.meta && it.at) it.meta.textContent = ago(it.at);
        attachIcon(it);
        it.el.classList.remove('hidden');
        g.sec.list.appendChild(it.el);
      });
      g.sec.el.classList.toggle('hidden', !g.picked.length);
      shown += g.picked.length;
    });

    // Typed text: "Go to …" on top when it looks like a URL, otherwise
    // "Search …" after the results (or alone when nothing matched).
    webGo.classList.add('hidden');
    webSearch.classList.add('hidden');
    var text = q.scope ? '' : q.text;
    if (text) {
      var url = PaletteRank.resolve(text, data.engine || '');
      if (PaletteRank.looksLikeUrl(text)) {
        showWeb(webGo, 'Go to ' + text, url, url, text);
        results.insertBefore(web, results.firstChild);
        web.classList.remove('trailing');
      } else {
        showWeb(webSearch, 'Search for “' + text + '”', engineHost(), url, text);
        results.insertBefore(web, empty);
        web.classList.toggle('trailing', shown > 0);
      }
      shown++;
    }
    web.classList.toggle('hidden', !text);
    empty.classList.toggle('hidden', shown > 0);

    results.scrollTop = 0;
    ui.setActive(0, false);
  }

  function move(delta) {
    var rows = ui.rows();
    if (!rows.length) return;
    var at = rows.indexOf(ui.active());
    ui.setActive(at < 0 ? 0 : Math.max(0, Math.min(rows.length - 1, at + delta)));
  }

  function openActive() {
    var row = ui.active();
    if (row && row.dataset.open) { ui.go(row.dataset.open); return; }
    // Nothing selectable (e.g. a prefix with no matches): typed text as is.
    var q = PaletteRank.parse(input.value);
    if (!q.scope && q.text) {
      var url = PaletteRank.resolve(q.text, data.engine || '');
      ui.go('omnibox://go?q=' + encodeURIComponent(q.text) + '&url=' + encodeURIComponent(url));
    }
  }

  input.addEventListener('input', render);
  // Runs before ui.js's document handler; preventDefault keeps it out.
  input.addEventListener('keydown', function (event) {
    if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
    switch (event.key) {
      case 'ArrowDown': event.preventDefault(); move(1); break;
      case 'ArrowUp': event.preventDefault(); move(-1); break;
      case 'PageDown': event.preventDefault(); move(5); break;
      case 'PageUp': event.preventDefault(); move(-5); break;
      case 'Tab': event.preventDefault(); move(event.shiftKey ? -1 : 1); break;
      case 'Enter': event.preventDefault(); openActive(); break;
      case 'Escape': event.preventDefault(); ui.go(document.body.dataset.escape); break;
    }
  });
  // Keep the caret in the input: clicks on rows open them, never focus.
  results.addEventListener('mousedown', function (event) { event.preventDefault(); });
  document.querySelector('.pal-foot').addEventListener('mousedown', function (event) { event.preventDefault(); });
  window.addEventListener('focus', function () { input.focus(); });

  render();
  input.focus();
})();
