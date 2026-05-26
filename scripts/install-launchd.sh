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
LABELS=(com.kway.dev.backend com.kway.dev.web com.kway.dev.backup)

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
        # Two-phase install so one label's transient bootstrap error
        # (commonly "Input/output error: 5" when launchd hasn't yet
        # reaped the previous instance) doesn't abort the rest.
        for label in "${LABELS[@]}"; do
            render_plist "$label"
        done
        # Unload everything first, then sleep to let launchd settle.
        for label in "${LABELS[@]}"; do
            launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
        done
        sleep 2
        for label in "${LABELS[@]}"; do
            # `launchctl bootstrap` is the modern (macOS 10.10+) replacement
            # for `launchctl load`.  Allow each one to fail independently
            # (we report the overall result via launchctl print below).
            if launchctl bootstrap "gui/$(id -u)" "$LA_DIR/${label}.plist" 2>/dev/null; then
                launchctl enable "gui/$(id -u)/$label" 2>/dev/null || true
                echo "  loaded $label"
            else
                echo "  WARNING: $label bootstrap failed — retrying once after sleep"
                sleep 3
                if launchctl bootstrap "gui/$(id -u)" "$LA_DIR/${label}.plist"; then
                    launchctl enable "gui/$(id -u)/$label" 2>/dev/null || true
                    echo "  loaded $label (after retry)"
                else
                    echo "  ERROR: $label still failed; check with: $0 status"
                fi
            fi
        done
        echo
        echo "  Agents installed.  They will run now and at each login."
        echo "  Logs:   $REPO_ROOT/logs/{backend,web,backup}.{out,err}.log"
        echo "  Backup: nightly at 03:00 → ~/kway-backups/YYYY-MM-DD/"
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
