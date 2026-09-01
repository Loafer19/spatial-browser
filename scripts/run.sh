#!/usr/bin/env bash
# Relaunch on crash (known CEF SPA soft-nav crash; session save ~1s debounce).
# No `set -e`: non-zero exit is the handled restart path.
set -uo pipefail

package="${1:?usage: scripts/run.sh <package>}"
shift

cd "$(dirname "$0")/.."
bin="target/bundle/$package"
if [ ! -x "$bin" ]; then
    echo "error: $bin not found or not executable — run scripts/bundle.sh $package first" >&2
    exit 1
fi

# Fast post-launch crashes look like a broken binary — give up after N.
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
