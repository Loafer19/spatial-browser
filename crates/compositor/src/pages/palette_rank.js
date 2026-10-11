// Ctrl+K palette ranking: pure functions (no DOM), used by palette.js and
// run under node by the unit test in palette.rs.
//
// A row is `{ t: title, u: url, k: keywords, r: history rank or -1 }`.
// Every query word must match one field. Per word: whole-field prefix >
// word-start substring > inner substring > subsequence (word starts and
// runs score higher, wide gaps lower, very scattered = no match). Title
// counts most, then keywords, then URL; newer history scores a bit higher.
var PaletteRank = (function () {
  'use strict';
  var ALNUM = /[\p{L}\p{N}]/u;
  var WEIGHTS = [1, 0.6, 0.7]; // title, url, keywords
  var RECENCY = 5;
  var HISTORY_SPAN = 300;

  function wordStart(s, i) {
    if (i <= 0) return true;
    var p = s.charAt(i - 1);
    var c = s.charAt(i);
    if (!ALNUM.test(p)) return true;
    // camelCase boundary: lower → upper.
    return p !== p.toUpperCase() && c !== c.toLowerCase();
  }

  function range(start, n) {
    var out = [];
    for (var i = 0; i < n; i++) out.push(start + i);
    return out;
  }

  // Greedy subsequence. With `prefer`, each char jumps ahead to a word
  // start (or stays on a run) when there is one. Jumps to a word start are
  // free (initials: "osb" → Open in System Browser); other gaps cost, and
  // too many skipped characters mean no match.
  function subsequence(s, l, w, prefer) {
    var hits = [];
    var from = 0;
    var score = 0;
    var skipped = 0;
    for (var j = 0; j < w.length; j++) {
      var c = w.charAt(j);
      var pick = -1;
      for (var x = from; x < l.length; x++) {
        if (l.charAt(x) !== c) continue;
        if (pick < 0) pick = x;
        if (!prefer) break;
        if (wordStart(s, x) || (hits.length && x === hits[hits.length - 1] + 1)) { pick = x; break; }
      }
      if (pick < 0) return null;
      var run = hits.length && pick === hits[hits.length - 1] + 1;
      var start = wordStart(s, pick);
      score += 1 + (run ? 5 : 0) + (start ? (pick === 0 ? 10 : 8) : 0);
      if (hits.length && !start) skipped += pick - hits[hits.length - 1] - 1;
      hits.push(pick);
      from = pick + 1;
    }
    if (skipped > w.length * 2 + 6) return null;
    var penalty = Math.min(15, skipped * 0.5);
    return { score: Math.max(1, score - penalty), hits: hits };
  }

  // One query word `w` (lowercase) in field `s` (`l` = lowercased).
  function matchWord(s, l, w) {
    var at = l.indexOf(w);
    if (at >= 0) {
      var best = at;
      for (var k = at; k >= 0; k = l.indexOf(w, k + 1)) {
        if (wordStart(s, k)) { best = k; break; }
      }
      var base = best === 0 ? 100 : wordStart(s, best) ? 80 : 50;
      return { score: base + w.length * 4, hits: range(best, w.length) };
    }
    return subsequence(s, l, w, true) || subsequence(s, l, w, false);
  }

  function words(text) {
    return String(text || '').trim().toLowerCase().split(/\s+/).filter(Boolean);
  }

  // { score, hits: [titleHits, urlHits, keywordHits] } or null.
  function score(item, ws) {
    var fields = [item.t || '', item.u || '', item.k || ''];
    var lower = fields.map(function (f) { return f.toLowerCase(); });
    var hits = [[], [], []];
    var total = 0;
    for (var i = 0; i < ws.length; i++) {
      var best = null;
      var bestField = -1;
      var bestScore = 0;
      for (var f = 0; f < fields.length; f++) {
        if (!fields[f]) continue;
        var m = matchWord(fields[f], lower[f], ws[i]);
        if (m && m.score * WEIGHTS[f] > bestScore) {
          best = m;
          bestField = f;
          bestScore = m.score * WEIGHTS[f];
        }
      }
      if (!best) return null;
      total += bestScore;
      hits[bestField] = hits[bestField].concat(best.hits);
    }
    if (typeof item.r === 'number' && item.r >= 0) {
      total += RECENCY * Math.max(0, 1 - item.r / HISTORY_SPAN);
    }
    return { score: total, hits: hits };
  }

  // `> text` = commands only, `@ text` = open pages only.
  function parse(raw) {
    var t = String(raw || '').replace(/^\s+/, '');
    if (t.charAt(0) === '>') return { scope: 'commands', text: t.slice(1).trim() };
    if (t.charAt(0) === '@') return { scope: 'pages', text: t.slice(1).trim() };
    return { scope: null, text: t.trim() };
  }

  var SCHEME = /^[a-z][a-z0-9+.-]*:\/\/\S/i;
  var HOST = /^(localhost|\d{1,3}(\.\d{1,3}){3}|\[[0-9a-f:.]+\]|([\p{L}\p{N}-]+\.)+[\p{L}]{2,})(:\d{1,5})?$/iu;

  function looksLikeUrl(text) {
    var t = String(text || '');
    if (!t || /\s/.test(t)) return false;
    if (SCHEME.test(t)) return true;
    return HOST.test(t.split(/[/?#]/)[0]);
  }

  // Where Enter on typed text goes: the URL itself, or a web search.
  function resolve(text, engine) {
    var t = String(text || '').trim();
    if (SCHEME.test(t)) return t;
    if (looksLikeUrl(t)) return (/^(localhost|127\.|\[)/i.test(t) ? 'http://' : 'https://') + t;
    return engine + encodeURIComponent(t);
  }

  return {
    words: words,
    score: score,
    parse: parse,
    looksLikeUrl: looksLikeUrl,
    resolve: resolve,
  };
})();
