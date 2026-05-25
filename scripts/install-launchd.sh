#!/usr/bin/env bash
# Install (or uninstall) the Kway Dev launchd agents into
# ~/Library/LaunchAgents/.  Idempotent: re-running just refreshes the
# templated paths and reloads.
#
# Usage:
#   ./scripts/install-launchd.sh install      install + load (default)
#   ./scripts/install-launchd.sh uninstall    unload + remove
#   ./scripts/install-launchd.sh status       show launchctl list output

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LA_DIR="$HOME/Library/LaunchAgents"
LABELS=(com.kway.dev.backend com.kway.dev.web)

mode="${1:-install}"

render_plist() {
    local label=$1
    local src="$REPO_ROOT/scripts/launchd/${label}.plist.template"
    local dst="$LA_DIR/${label}.plist"
    [[ -f "$src" ]] || { echo "template not found: $src" >&2; exit 1; }
    sed -e "s|@REPO_ROOT@|$REPO_ROOT|g" -e "s|@USER_HOME@|$HOME|g" "$src" > "$dst"
    echo "  wrote $dst"
}

case "$mode" in
    install)
        mkdir -p "$LA_DIR" "$REPO_ROOT/logs"
        chmod +x "$REPO_ROOT/scripts/launchd/"*.sh
        for label in "${LABELS[@]}"; do
            render_plist "$label"
            # `launchctl bootstrap` is the modern (macOS 10.10+) replacement
            # for `launchctl load`.  Unload first so re-runs pick up edits.
            launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
            launchctl bootstrap "gui/$(id -u)" "$LA_DIR/${label}.plist"
            launchctl enable "gui/$(id -u)/$label"
            echo "  loaded $label"
        done
        echo
        echo "  Agents installed.  They will run now and at each login."
        echo "  Logs:   $REPO_ROOT/logs/{backend,web}.{out,err}.log"
        echo "  Status: $0 status"
        ;;
    uninstall)
        for label in "${LABELS[@]}"; do
            launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
            rm -f "$LA_DIR/${label}.plist"
            echo "  removed $label"
        done
        ;;
    status)
        for label in "${LABELS[@]}"; do
            echo "── $label ──"
            launchctl print "gui/$(id -u)/$label" 2>&1 | head -20 || echo "  (not loaded)"
            echo
        done
        ;;
    *)
        echo "usage: $0 [install|uninstall|status]" >&2
        exit 2
        ;;
esac
