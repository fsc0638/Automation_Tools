#!/usr/bin/env bash
# Manual start/stop helper for Kway Dev (host-run mode).
#
# Use this when launchd is NOT installed (install.sh --no-launchd) or
# when you've stopped the agents and want to bring things up by hand.
# In launchd mode you should just let the agents handle restarts.
#
# Usage:
#   ./scripts/start-all.sh          start all services
#   ./scripts/start-all.sh --stop   stop backend + web (Postgres left running)
#   ./scripts/start-all.sh --build  rebuild backend + web before starting

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BACKEND_BIN="$REPO_ROOT/backend/target/release/kway-dev-backend"
LOG_DIR="$REPO_ROOT/logs"
mkdir -p "$LOG_DIR"

mode="start"
do_build=0
for arg in "$@"; do
    case "$arg" in
        --stop)  mode=stop ;;
        --build) do_build=1 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

stop_backend() {
    pkill -x kway-dev-backend 2>/dev/null && echo "  stopped backend" || echo "  backend not running"
}
stop_web() {
    # next start runs as `node`; find by listening port instead.
    local pid
    pid=$(lsof -nP -iTCP:3000 -sTCP:LISTEN -t 2>/dev/null || true)
    [[ -n "$pid" ]] && { kill "$pid" && echo "  stopped web (pid $pid)"; } || echo "  web not running on :3000"
}

if [[ "$mode" == stop ]]; then
    echo "[stop] backend + web"
    stop_backend
    stop_web
    echo "[stop] Postgres container left running — use 'docker compose stop postgres' to stop it"
    exit 0
fi

# ── Postgres ──────────────────────────────────────────────────────────────
echo "[db] ensuring Postgres container is up"
docker compose -f "$REPO_ROOT/docker-compose.yml" up -d postgres >/dev/null
for _ in $(seq 1 20); do
    docker compose -f "$REPO_ROOT/docker-compose.yml" exec -T postgres pg_isready -U postgres >/dev/null 2>&1 && break
    sleep 1
done

# ── Backend ───────────────────────────────────────────────────────────────
if [[ $do_build -eq 1 ]]; then
    echo "[backend] cargo build --release"
    (cd "$REPO_ROOT/backend" && CARGO_TARGET_DIR="$REPO_ROOT/backend/target" cargo build --release)
fi
if pgrep -x kway-dev-backend >/dev/null; then
    echo "[backend] already running"
else
    [[ -x "$BACKEND_BIN" ]] || { echo "backend binary not found at $BACKEND_BIN — run install.sh or pass --build" >&2; exit 1; }
    echo "[backend] starting → $LOG_DIR/backend.{out,err}.log"
    (cd "$REPO_ROOT/backend"
     set -o allexport; source .env; set +o allexport
     nohup "$BACKEND_BIN" >>"$LOG_DIR/backend.out.log" 2>>"$LOG_DIR/backend.err.log" &
     echo "  pid $!"
    )
fi

# ── Web ───────────────────────────────────────────────────────────────────
if [[ $do_build -eq 1 ]]; then
    echo "[web] npm install && npm run build"
    (cd "$REPO_ROOT/web" && npm install --silent && npm run build)
fi
if lsof -nP -iTCP:3000 -sTCP:LISTEN >/dev/null 2>&1; then
    echo "[web] already running on :3000"
else
    echo "[web] starting → $LOG_DIR/web.{out,err}.log"
    (cd "$REPO_ROOT/web"
     nohup npx next start -p 3000 >>"$LOG_DIR/web.out.log" 2>>"$LOG_DIR/web.err.log" &
     echo "  pid $!"
    )
fi

echo
echo "  Web:     http://localhost:3000"
echo "  Backend: http://127.0.0.1:8080/api"
echo "  Logs:    $LOG_DIR/"
