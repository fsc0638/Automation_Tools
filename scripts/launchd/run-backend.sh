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

mkdir -p "$REPO_ROOT/logs"

exec "$REPO_ROOT/backend/target/release/kway-dev-backend"
