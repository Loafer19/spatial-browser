#!/usr/bin/env bash
# Runs a bundled binary, auto-relaunching it if it crashes or exits
# non-zero. CEF has a known unresolved crash on SPA-style client-side
# navigation (TabInterface::GetFromContents returning null inside
# ReadAnythingSoftNavigationObserver — see cef-bridge's
# on_before_command_line_processing) that takes down the whole browser
# process, not just the page that triggered it — there's no per-tab
# process isolation to fall back on here, since it's a crash in
# browser-process code reacting to a renderer's IPC message, not a
# renderer crash. Restarting loses at most the last unsaved second of
# canvas state (session.json saves debounced ~1/sec), not the session.
#
# Not `set -e`: a non-zero/crash exit from the binary is the expected,
# handled case below, not a script failure.
set -uo pipefail

package="${1:?usage: scripts/run.sh <package>}"
shift

cd "$(dirname "$0")/.."
bin="target/bundle/$package"
if [ ! -x "$bin" ]; then
    echo "error: $bin not found or not executable — run scripts/bundle.sh $package first" >&2
    exit 1
fi

# A crash under this many seconds after launch doesn't fit the known
# crash above (that one only happens after real browsing) — it's more
# likely an actually-broken binary (bad build, missing libs), so repeat
# fast crashes give up instead of spinning forever.
fast_crash_seconds=5
max_consecutive_fast_crashes=5
fast_crashes=0

while true; do
    start=$(date +%s)
    "$bin" "$@"
    code=$?
    if [ "$code" -eq 0 ]; then
        echo "$package exited normally"
        exit 0
    fi

    elapsed=$(( $(date +%s) - start ))
    echo "$package exited with code $code after ${elapsed}s" >&2

    if [ "$elapsed" -lt "$fast_crash_seconds" ]; then
        fast_crashes=$(( fast_crashes + 1 ))
        if [ "$fast_crashes" -ge "$max_consecutive_fast_crashes" ]; then
            echo "error: $package crashed $fast_crashes times within ${fast_crash_seconds}s of launch each time — giving up (looks broken, not the known CEF crash)" >&2
            exit "$code"
        fi
        sleep 2
    else
        fast_crashes=0
    fi
    echo "restarting $package..." >&2
done
