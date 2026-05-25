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

exec npx next start -p 3000
