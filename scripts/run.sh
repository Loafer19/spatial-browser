#!/usr/bin/env bash
# Relaunch on crash (known CEF SPA soft-nav crash; session save ~1s debounce).
# No `set -e`: non-zero exit is the handled restart path.
#
# Appends one line per relaunch to ~/.config/spatial-browser/restarts.log so
# VAAPI-off / CEF bumps can be judged from restart rate over time.
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
# Restarts this wrapper session (excludes the initial launch).
restart_count=0

# Match compositor paths: always $HOME/.config/spatial-browser (not XDG_CONFIG_HOME).
config_dir="$HOME/.config/spatial-browser"
log_file="$config_dir/restarts.log"
mkdir -p "$config_dir"

# One JSON-ish line per event (easy to grep/count later).
log_restart() {
    local code=$1 elapsed=$2 reason=$3
    local ts
    ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    printf '%s package=%s exit=%s elapsed_s=%s reason=%s restart_n=%s consecutive_fast=%s\n' \
        "$ts" "$package" "$code" "$elapsed" "$reason" "$restart_count" "$fast_crashes" \
        >>"$log_file"
}

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
        reason="fast_crash"
        if [ "$fast_crashes" -ge "$max_consecutive_fast_crashes" ]; then
            restart_count=$(( restart_count + 1 ))
            log_restart "$code" "$elapsed" "give_up_fast_crash"
            echo "error: $package crashed $fast_crashes times within ${fast_crash_seconds}s of launch each time — giving up (looks broken, not the known CEF crash)" >&2
            echo "restarts logged to $log_file" >&2
            exit "$code"
        fi
        sleep 2
    else
        fast_crashes=0
        # Longer-lived process dying is the known CEF SPA soft-nav path.
        reason="cef_soft_nav"
    fi

    restart_count=$(( restart_count + 1 ))
    log_restart "$code" "$elapsed" "$reason"
    echo "restarting $package... (restart #$restart_count, reason=$reason) — log: $log_file" >&2
done
