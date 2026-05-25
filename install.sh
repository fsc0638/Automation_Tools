#!/usr/bin/env bash
# Kway Dev — Mac Mini one-shot installer.
#
# Usage:
#   ./install.sh                  interactive (asks host vs docker)
#   ./install.sh --mode=host      Mac Mini production baseline (DMG enabled)
#   ./install.sh --mode=docker    container-only (DMG disabled)
#   ./install.sh --check          pre-flight checks only, no changes
#   ./install.sh --no-launchd     host mode: build & start, skip auto-start at login
#
# Designed to be idempotent: running twice does NOT regenerate secrets
# (which would invalidate every existing user's vault) and does NOT
# overwrite an existing backend/.env.

set -euo pipefail

# ── Configuration knobs ───────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
DEFAULT_PROJECT_DATA_ROOT="${HOME}/kway-project-data"
DEFAULT_DMG_ROOT="${HOME}/kway-dmg-store"
DEFAULT_DMG_SIZE_MB=4096

# Colour helpers (no-op when stdout isn't a TTY).
if [[ -t 1 ]]; then
    C_OK="\033[32m"; C_WARN="\033[33m"; C_ERR="\033[31m"; C_DIM="\033[2m"; C_OFF="\033[0m"
else
    C_OK=""; C_WARN=""; C_ERR=""; C_DIM=""; C_OFF=""
fi
ok()   { printf "${C_OK}✓${C_OFF} %s\n" "$*"; }
warn() { printf "${C_WARN}!${C_OFF} %s\n" "$*"; }
err()  { printf "${C_ERR}✗${C_OFF} %s\n" "$*" >&2; }
hint() { printf "${C_DIM}  %s${C_OFF}\n" "$*"; }
step() { printf "\n${C_OK}══ %s ══${C_OFF}\n" "$*"; }

# ── Parse flags ───────────────────────────────────────────────────────────
MODE=""
DRY_RUN=0
SKIP_LAUNCHD=0
for arg in "$@"; do
    case "$arg" in
        --mode=host)   MODE=host ;;
        --mode=docker) MODE=docker ;;
        --check)       DRY_RUN=1 ;;
        --no-launchd)  SKIP_LAUNCHD=1 ;;
        -h|--help)
            sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) err "unknown flag: $arg"; exit 2 ;;
    esac
done

# ── Pre-flight checks ─────────────────────────────────────────────────────
step "Pre-flight checks"

if [[ "$(uname)" != "Darwin" ]]; then
    err "this installer targets macOS (Mac Mini deployment). For Linux/Windows use docker-compose.yml directly."
    exit 1
fi
ok "macOS $(sw_vers -productVersion)"

# Homebrew is the standard route for installing rust/node on host mode.
if command -v brew >/dev/null 2>&1; then
    ok "Homebrew found at $(command -v brew)"
else
    warn "Homebrew not installed — host mode will not be able to install rust/node automatically"
    hint "install from https://brew.sh and re-run"
fi

# Docker Desktop is required for Postgres regardless of mode.
if docker info >/dev/null 2>&1; then
    ok "Docker daemon running"
else
    err "Docker is not running — start Docker Desktop and re-run"
    exit 1
fi

# Tailscale: warn only; not strictly required for local install but
# needed for remote access to the Mac Mini from the user's laptop / iPhone.
if command -v tailscale >/dev/null 2>&1 && tailscale status >/dev/null 2>&1; then
    # grep -o with no match returns 1; let pipefail's exit silently fall
    # through so the parent `if` branch keeps running.
    ts_hostname=$(tailscale status --json 2>/dev/null | (grep -o '"DNSName":"[^"]*"' || true) | head -1 | cut -d'"' -f4)
    ok "Tailscale up (${ts_hostname:-no hostname})"
else
    warn "Tailscale not configured — clients won't be able to reach this Mac Mini remotely"
    hint "brew install --cask tailscale  &&  open -a Tailscale"
fi

# Hermes / OpenClaw gateways: warn only, AI features need them but install
# can proceed without.
check_gateway() {
    local name=$1 url=$2
    if curl -fsS --max-time 2 "$url" >/dev/null 2>&1; then
        ok "$name reachable at $url"
    else
        warn "$name not reachable at $url — AI features will be unavailable until you start it"
    fi
}
check_gateway "Hermes Gateway"   "http://127.0.0.1:8642/v1/models"
check_gateway "OpenClaw Gateway" "http://127.0.0.1:18789/v1/models"

if [[ $DRY_RUN -eq 1 ]]; then
    step "Dry run complete — no changes made"
    exit 0
fi

# ── Pick mode ─────────────────────────────────────────────────────────────
if [[ -z "$MODE" ]]; then
    step "Choose deployment mode"
    echo "  [1] host    — backend runs natively on macOS (enables DMG per-user encryption — recommended for Mac Mini)"
    echo "  [2] docker  — backend runs in a container (no DMG layer; vault encryption still active)"
    read -r -p "Pick mode [1/2] (default: 1): " ans
    case "${ans:-1}" in
        1|host)   MODE=host ;;
        2|docker) MODE=docker ;;
        *) err "invalid choice"; exit 2 ;;
    esac
fi
ok "mode: $MODE"

# ── Generate secrets / backend/.env ───────────────────────────────────────
step "Backend configuration"

BACKEND_ENV="$REPO_ROOT/backend/.env"
if [[ -f "$BACKEND_ENV" ]]; then
    ok "$BACKEND_ENV already exists, leaving secrets untouched"
    warn "If you ever regenerate JWT_SECRET or GIT_TOKEN_ENCRYPTION_KEY, all existing user data becomes unrecoverable."
else
    JWT_SECRET=$(openssl rand -hex 48)
    GIT_TOKEN_ENCRYPTION_KEY=$(openssl rand -base64 32)
    cp "$REPO_ROOT/backend/.env.example" "$BACKEND_ENV"
    # macOS sed needs '' for in-place no-suffix.
    sed -i '' "s|^JWT_SECRET=.*|JWT_SECRET=${JWT_SECRET}|" "$BACKEND_ENV"
    sed -i '' "s|^GIT_TOKEN_ENCRYPTION_KEY=.*|GIT_TOKEN_ENCRYPTION_KEY=${GIT_TOKEN_ENCRYPTION_KEY}|" "$BACKEND_ENV"

    if [[ "$MODE" == host ]]; then
        sed -i '' "s|^PROJECT_DATA_ROOT=.*|PROJECT_DATA_ROOT=${DEFAULT_PROJECT_DATA_ROOT}|" "$BACKEND_ENV"
        sed -i '' "s|^DMG_ROOT=.*|DMG_ROOT=${DEFAULT_DMG_ROOT}|" "$BACKEND_ENV"
        sed -i '' "s|^DMG_SIZE_MB=.*|DMG_SIZE_MB=${DEFAULT_DMG_SIZE_MB}|" "$BACKEND_ENV"
        sed -i '' "s|^SERVER_HOST=.*|SERVER_HOST=127.0.0.1|" "$BACKEND_ENV"
    fi
    ok "generated $BACKEND_ENV with fresh JWT_SECRET + GIT_TOKEN_ENCRYPTION_KEY"
    warn "BACK THIS FILE UP — losing these secrets bricks all existing vault data."
fi

# web/.env.local
WEB_ENV="$REPO_ROOT/web/.env.local"
if [[ ! -f "$WEB_ENV" ]]; then
    echo "NEXT_PUBLIC_API_URL=http://localhost:8080/api" > "$WEB_ENV"
    ok "wrote $WEB_ENV"
fi

# ── Filesystem layout ─────────────────────────────────────────────────────
step "Filesystem layout"
mkdir -p "$DEFAULT_PROJECT_DATA_ROOT/users"
ok "$DEFAULT_PROJECT_DATA_ROOT/users"
if [[ "$MODE" == host ]]; then
    mkdir -p "$DEFAULT_DMG_ROOT"
    chmod 700 "$DEFAULT_DMG_ROOT"
    ok "$DEFAULT_DMG_ROOT (mode 700)"
fi

# ── Postgres (always docker) ──────────────────────────────────────────────
step "Postgres"
docker compose -f "$REPO_ROOT/docker-compose.yml" up -d postgres
# Wait for healthy.
for _ in $(seq 1 30); do
    if docker compose -f "$REPO_ROOT/docker-compose.yml" exec -T postgres pg_isready -U postgres >/dev/null 2>&1; then
        ok "Postgres healthy"
        break
    fi
    sleep 1
done

# ── Backend ───────────────────────────────────────────────────────────────
step "Backend"
if [[ "$MODE" == host ]]; then
    if ! command -v cargo >/dev/null 2>&1; then
        err "rust toolchain not installed"
        hint "brew install rust    # or  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
    ok "cargo found at $(command -v cargo)"
    (cd "$REPO_ROOT/backend" && CARGO_TARGET_DIR="$REPO_ROOT/backend/target" cargo build --release)
    ok "backend built → $REPO_ROOT/backend/target/release/kway-dev-backend"
    hint "first run will auto-apply migrations 0001..0049 and create vault_keys system_v1"
else
    docker compose -f "$REPO_ROOT/docker-compose.yml" build backend
    docker compose -f "$REPO_ROOT/docker-compose.yml" up -d backend
    ok "backend container up at http://localhost:8080"
fi

# ── Web ───────────────────────────────────────────────────────────────────
step "Web frontend"
if [[ "$MODE" == host ]]; then
    if ! command -v node >/dev/null 2>&1; then
        err "Node.js not installed"
        hint "brew install node"
        exit 1
    fi
    ok "node found at $(command -v node)"
    (cd "$REPO_ROOT/web" && npm install --silent && npm run build)
    ok "web built → $REPO_ROOT/web/.next"
else
    docker compose -f "$REPO_ROOT/docker-compose.yml" build web
    docker compose -f "$REPO_ROOT/docker-compose.yml" up -d web
    ok "web container up at http://localhost:3000"
fi

# ── launchd (host mode only) ──────────────────────────────────────────────
if [[ "$MODE" == host && $SKIP_LAUNCHD -eq 0 ]]; then
    step "launchd auto-start"
    "$REPO_ROOT/scripts/install-launchd.sh" install
    ok "backend + web will auto-start on login (loaded into ~/Library/LaunchAgents/)"
elif [[ "$MODE" == host && $SKIP_LAUNCHD -eq 1 ]]; then
    warn "launchd skipped — start backend with: $REPO_ROOT/scripts/start-all.sh"
fi

# ── Summary ───────────────────────────────────────────────────────────────
step "All done"
cat <<EOF

  Web URL:       http://localhost:3000
                 (over Tailscale: https://<your-tailnet-hostname>/)
  Backend API:   http://127.0.0.1:8080/api
  Postgres:      127.0.0.1:5432  (user=postgres, db=kway_dev)

  Config:        $BACKEND_ENV
                 ${C_WARN}back this up — losing the secrets here bricks all vault data${C_OFF}

  Next steps:
    1. Open the Web URL and register the first user (auto-provisions DMG)
    2. If Tailscale / Hermes / OpenClaw warnings appeared above, address them
    3. To stop:    $REPO_ROOT/scripts/start-all.sh --stop
       To restart: $REPO_ROOT/scripts/start-all.sh

EOF
