#!/usr/bin/env python3
"""Minimal Qt WebEngine smoke test for Twitch / codec support.

Not part of spatial-browser — throwaway prototype to see whether the
system/Qt Chromium build can play Twitch (H.264) where CEF minimal cannot.

Usage (from repo root):
  scratch/qt-webengine-venv/bin/python tools/qt_webengine_twitch_smoke.py
  scratch/qt-webengine-venv/bin/python tools/qt_webengine_twitch_smoke.py 'https://www.twitch.tv/<channel>'
"""

from __future__ import annotations

import sys

from PySide6.QtCore import QUrl, Qt
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (
    QApplication,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QPushButton,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)
from PySide6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEngineSettings
from PySide6.QtWebEngineWidgets import QWebEngineView

DEFAULT_URL = "https://www.twitch.tv/"

CODEC_PROBE_JS = r"""
(function () {
  const types = [
    'video/mp4; codecs="avc1.42E01E"',
    'video/mp4; codecs="avc1.4D401F"',
    'video/mp4; codecs="avc1.640028"',
    'audio/mp4; codecs="mp4a.40.2"',
    'video/webm; codecs="vp9"',
    'video/webm; codecs="vp8"',
    'video/mp4; codecs="av01.0.05M.08"',
    'application/vnd.apple.mpegurl',
  ];
  const mse = window.MediaSource || window.WebKitMediaSource;
  const lines = [];
  lines.push('userAgent: ' + navigator.userAgent);
  lines.push('MediaSource: ' + (mse ? 'yes' : 'no'));
  if (mse && mse.isTypeSupported) {
    for (const t of types) {
      lines.push((mse.isTypeSupported(t) ? 'OK  ' : 'NO  ') + t);
    }
  }
  if (window.HTMLMediaElement && HTMLMediaElement.prototype.canPlayType) {
    const v = document.createElement('video');
    lines.push('canPlayType avc1.42E01E: ' + JSON.stringify(v.canPlayType('video/mp4; codecs="avc1.42E01E"')));
    lines.push('canPlayType mp4a.40.2: ' + JSON.stringify(v.canPlayType('audio/mp4; codecs="mp4a.40.2"')));
    lines.push('canPlayType vp9: ' + JSON.stringify(v.canPlayType('video/webm; codecs="vp9"')));
  }
  return lines.join('\n');
})();
"""


class Main(QMainWindow):
    def __init__(self, start_url: str) -> None:
        super().__init__()
        self.setWindowTitle("Qt WebEngine Twitch smoke test")
        self.resize(1280, 800)

        profile = QWebEngineProfile.defaultProfile()
        settings = profile.settings()
        settings.setAttribute(QWebEngineSettings.WebAttribute.PlaybackRequiresUserGesture, False)
        settings.setAttribute(QWebEngineSettings.WebAttribute.JavascriptEnabled, True)
        settings.setAttribute(QWebEngineSettings.WebAttribute.LocalStorageEnabled, True)

        self.view = QWebEngineView()
        self.page = QWebEnginePage(profile, self.view)
        self.view.setPage(self.page)

        self.url_edit = QLineEdit(start_url)
        go = QPushButton("Go")
        go.clicked.connect(self.navigate)
        self.url_edit.returnPressed.connect(self.navigate)

        probe = QPushButton("Probe codecs")
        probe.clicked.connect(self.probe_codecs)

        top = QHBoxLayout()
        top.addWidget(self.url_edit, stretch=1)
        top.addWidget(go)
        top.addWidget(probe)

        self.log = QTextEdit()
        self.log.setReadOnly(True)
        self.log.setMaximumHeight(180)
        self.log.setFont(QFont("monospace", 10))
        self.log.setPlaceholderText("Codec probe output appears here…")

        hint = QLabel(
            "Smoke test only. If H.264 (avc1) shows OK and Twitch plays here, "
            "Qt WebEngine on this machine has the codecs CEF minimal lacks."
        )
        hint.setWordWrap(True)

        root = QVBoxLayout()
        root.addWidget(hint)
        root.addLayout(top)
        root.addWidget(self.view, stretch=1)
        root.addWidget(self.log)

        w = QWidget()
        w.setLayout(root)
        self.setCentralWidget(w)

        self.page.loadFinished.connect(self.on_loaded)
        self.navigate()

    def navigate(self) -> None:
        url = self.url_edit.text().strip() or DEFAULT_URL
        if not url.startswith("http"):
            url = "https://" + url
        self.url_edit.setText(url)
        self.view.setUrl(QUrl(url))
        self.log.append(f"→ loading {url}")

    def on_loaded(self, ok: bool) -> None:
        self.log.append(f"loadFinished ok={ok} title={self.view.title()!r}")
        # Auto-probe once after first load.
        if ok and self.log.toPlainText().count("userAgent:") == 0:
            self.probe_codecs()

    def probe_codecs(self) -> None:
        self.page.runJavaScript(CODEC_PROBE_JS, self._on_probe)

    def _on_probe(self, result) -> None:
        self.log.append("--- codec probe ---")
        self.log.append(str(result) if result is not None else "(null result)")
        self.log.append("-------------------")


def main() -> int:
    # Must exist before any QWebEngine* usage on some platforms.
    QApplication.setAttribute(Qt.ApplicationAttribute.AA_ShareOpenGLContexts, True)
    app = QApplication(sys.argv)
    start = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_URL
    win = Main(start)
    win.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
