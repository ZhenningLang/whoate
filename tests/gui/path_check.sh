#!/usr/bin/env bash
# QA12: launchd-style PATH. A pane on a private tmux server (temporary
# TMUX_TMPDIR, never the default server) must appear in the bundled app's
# dump-tick only when the app repairs PATH; the control run proves the test
# can fail. The app starts through launch.py (own coalition, launchd-style
# environment), never as a child of this shell.
# Usage: tests/gui/path_check.sh <Whoate executable> <evidence dir>
set -euo pipefail
exec </dev/null
binary=$1
out=$2
tmux_bin=$(PATH=/opt/homebrew/bin:/usr/local/bin:$PATH command -v tmux)
# sockaddr_un is short: keep the socket directory short and private.
tmp=$(mktemp -d /private/tmp/wg-XXXXXX)
cleanup() {
    env -i TMUX_TMPDIR="$tmp" "$tmux_bin" kill-server 2>/dev/null || true
    rm -rf "$tmp"
    echo "QA12 cleanup server+socket removed=$([[ -e $tmp ]] && echo no || echo yes)"
}
trap cleanup EXIT
mkdir -p "$tmp/home" "$tmp/cache"
env -i TMUX_TMPDIR="$tmp" "$tmux_bin" new-session -d -s whoatepath "/bin/sleep 600"
pane=$(env -i TMUX_TMPDIR="$tmp" "$tmux_bin" list-panes -a -F '#{pane_id}' | head -1)
[[ $pane =~ ^%[0-9]+$ ]] || { echo "QA12_PANE_MISSING"; exit 1; }

launcher="$(cd "$(dirname "$0")" && pwd)/launch.py"
run() {
    python3 -B "$launcher" run "$binary" --timeout 120 --env PATH=/usr/bin:/bin:/usr/sbin:/sbin \
        --env HOME="$tmp/home" --env TMUX_TMPDIR="$tmp" --env WHOATE_HOME_OVERRIDE="$tmp/home" \
        --env WHOATE_CACHE_DIR="$tmp/cache" --env WHOATE_RULES="$tmp/rules.toml" \
        --env WHOATE_UI_AUTOMATION="$1" >"$2"
}
run dump-tick "$out/dump-tick.json"
run dump-tick,no-path-repair "$out/dump-tick-control.json"

python3 - "$pane" "$out/dump-tick.json" "$out/dump-tick-control.json" <<'PY'
import json, sys
pane, repaired, control = sys.argv[1], *map(lambda p: json.load(open(p)), sys.argv[2:])
def found(report):
    return [o["id"] for o in report["owners"]
            if o["kind"] == "tmux_pane" and (o.get("origin") or {}).get("tmux_pane") == pane]
a, b = found(repaired), found(control)
print(f"QA12 pane={pane} repaired_owners={a} control_owners={b}")
sys.exit(0 if a and not b else 1)
PY
