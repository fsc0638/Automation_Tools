#!/usr/bin/env bash
# Wrapper invoked by com.kway.dev.backend launchd plist.
# Sources backend/.env so DATABASE_URL etc. land in the process env
# (launchd ignores shell rc files), then exec's the built binary.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT/backend"

set -o allexport
# shellcheck disable=SC1091
source .env
set +o allexport

# Tailscale-only binding (Phase 1 — internal + 關係企業 deployment).
#
# When SERVER_HOST=auto-tailscale, override it with the live Tailscale IP
# so the listening socket is reachable only over the Tailscale tunnel,
# not from the office LAN or any future public IP this host gets.
# Falls back to 127.0.0.1 (loopback only) when Tailscale is down so we
# fail closed: a misconfigured/disconnected node never accidentally
# serves the world.
if [[ "${SERVER_HOST:-}" == "auto-tailscale" ]]; then
    ts_ip=$(tailscale ip -4 2>/dev/null | head -1 || true)
    if [[ -n "$ts_ip" ]]; then
        export SERVER_HOST="$ts_ip"
        echo "run-backend: SERVER_HOST=auto-tailscale resolved to $ts_ip" >&2
    else
        export SERVER_HOST="127.0.0.1"
        echo "run-backend: WARNING: Tailscale not up, falling back to 127.0.0.1 only" >&2
    fi
fi

mkdir -p "$REPO_ROOT/logs"

exec "$REPO_ROOT/backend/target/release/kway-dev-backend"
