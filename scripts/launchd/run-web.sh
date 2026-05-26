#!/usr/bin/env bash
# Wrapper invoked by com.kway.dev.web launchd plist.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT/web"

if [[ -f .env.local ]]; then
    set -o allexport
    # shellcheck disable=SC1091
    source .env.local
    set +o allexport
fi

mkdir -p "$REPO_ROOT/logs"

# Tailscale-only binding (Phase 1 — internal + 關係企業 deployment).
# Bind Next.js to the live Tailscale IP so the office LAN can't reach
# :3000 directly. Falls back to 127.0.0.1 (loopback only) if Tailscale
# is down, so we never accidentally serve the world.
ts_ip=$(tailscale ip -4 2>/dev/null | head -1 || true)
bind_host="${ts_ip:-127.0.0.1}"
echo "run-web: binding to $bind_host:3000" >&2

exec npx next start -H "$bind_host" -p 3000
