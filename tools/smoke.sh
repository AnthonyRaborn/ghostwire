#!/usr/bin/env bash
# Smoke-tests the real binary end to end. Runs --demo in a tmux pane, drives it with a
# few keys (skip the boot, dive into NETSTATUS, open and close the manual, jack out),
# and checks the screen after each, that it exited cleanly, and that it handed the
# terminal back. Then checks that --check-config runs. Needs tmux.
#
#   tools/smoke.sh [path/to/ghostwire]
set -euo pipefail

bin=$(realpath "${1:-target/debug/ghostwire}")
work=$(mktemp -d)
tmux=(tmux -L "ghostwire-smoke-$$" -f /dev/null)
trap '"${tmux[@]}" kill-server 2>/dev/null || true; rm -rf "$work"' EXIT
# Keep the run's config, cache, and log out of the real ones.
export XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
unset RUST_BACKTRACE RUST_LIB_BACKTRACE

fail() {
    echo "smoke: $*" >&2
    if [[ -n ${screen:-} ]]; then
        printf '%s\n' "--- last screen ---" "$screen" >&2
    fi
    exit 1
}

capture() {
    screen=$("${tmux[@]}" capture-pane -p -t smoke 2>/dev/null || true)
}

# Waits up to `seconds` for `text` to be on screen (or, with `!`, to be gone),
# pressing `key` (if given) every couple of seconds meanwhile.
wait_for() {
    local seconds=$1 text=$2 key=${3:-} gone=false
    if [[ $text == '!'* ]]; then
        gone=true
        text=${text:1}
    fi
    local deadline=$((SECONDS + seconds)) next_key=$SECONDS
    while ((SECONDS < deadline)); do
        if [[ -n $key ]] && ((SECONDS >= next_key)); then
            "${tmux[@]}" send-keys -t smoke -l "$key"
            next_key=$((SECONDS + 2))
        fi
        capture
        local present=false
        [[ $screen == *"$text"* ]] && present=true
        if [[ $present != "$gone" ]]; then
            return 0
        fi
        sleep 0.5
    done
    if $gone; then
        fail "\"$text\" still on screen after ${seconds}s"
    fi
    fail "never drew \"$text\" in ${seconds}s"
}

"${tmux[@]}" new-session -d -s smoke -x 132 -y 36 \
    "'$bin' --demo; echo \$? > '$work/status'; sleep 30"
"${tmux[@]}" pipe-pane -o -t smoke "cat >> '$work/raw'"

# Any key skips the boot log; after it, x does nothing.
wait_for 15 "ZAIBATSU INDEX" x
# Demo fetches fail now and then on purpose, so keep pressing 6 until NETSTATUS has
# something to show; a repeat press is a no-op once it's diving.
wait_for 20 "DIVE // NETSTATUS" 6
wait_for 5 "TERMINATOR //"
wait_for 5 "RIG MANUAL" '?'
"${tmux[@]}" send-keys -t smoke -l x
wait_for 5 "!RIG MANUAL"
"${tmux[@]}" send-keys -t smoke -l q

for _ in $(seq 20); do
    [[ -s $work/status ]] && break
    sleep 0.5
done
[[ -s $work/status ]] || fail "still running 10s after q"
status=$(<"$work/status")
[[ $status == 0 ]] || fail "--demo exited with status $status"
if grep -qF "panicked" "$work/raw"; then
    fail "panicked"
fi
# The last thing on the way out is leaving the alternate screen.
last=$(grep -ao $'\e\\[?1049[hl]' "$work/raw" | tail -1 || true)
[[ $last == $'\e[?1049l' ]] || fail "didn't leave the alternate screen on exit"

report=$("$bin" --check-config) || fail "--check-config exited with status $?"
[[ $report == *"sources will run."* ]] || fail "--check-config printed no summary"

echo "smoke: ok"
