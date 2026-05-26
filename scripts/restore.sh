#!/usr/bin/env bash
# Kway Dev — restore from a backup.sh-produced directory.
#
# Usage:
#   ./scripts/restore.sh --check <backup-dir>      verify manifest checksums
#   ./scripts/restore.sh --dry-run <backup-dir>    show what would happen
#   ./scripts/restore.sh --apply <backup-dir>      actually restore (destructive)
#
# What --apply does, in order, all-or-nothing within reason:
#   1. Stops backend + web launchd agents (so no new writes during restore)
#   2. Force-unmounts every per-user DMG
#   3. Restores backend/.env (after backing up the current one to .env.pre-restore)
#   4. DROPs the kway_dev DB and pg_restores the dump
#   5. Restores sparseimages to DMG_ROOT
#   6. Restarts backend (which will auto-mount as users log back in)
#
# Refuses to overwrite without --apply. Always backs up the current
# state to <backup-dir>.pre-restore-<timestamp> before mutating anything,
# so an aborted restore can be wound back.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG_ROOT="${DMG_ROOT:-${HOME}/kway-dmg-store}"
PROJECT_DATA_ROOT="${PROJECT_DATA_ROOT:-${HOME}/kway-project-data}"

log() { printf "[%s] %s\n" "$(date +%Y-%m-%dT%H:%M:%S%z)" "$*"; }
fatal() { log "FATAL: $*"; exit 1; }

mode=""
backup_dir=""
for arg in "$@"; do
    case "$arg" in
        --check)   mode=check ;;
        --dry-run) mode=dry ;;
        --apply)   mode=apply ;;
        -h|--help)
            sed -n '2,18p' "$0" | sed 's|^# \{0,1\}||'
            exit 0
            ;;
        *)
            if [[ -z "$backup_dir" ]]; then
                backup_dir="$arg"
            else
                fatal "unexpected argument: $arg"
            fi
            ;;
    esac
done

[[ -z "$mode" ]] && fatal "specify one of --check / --dry-run / --apply (see --help)"
[[ -z "$backup_dir" ]] && fatal "backup directory not given"
backup_dir="$(cd "$backup_dir" && pwd)" 2>/dev/null || fatal "$backup_dir: not a directory"
[[ -f "$backup_dir/manifest.txt" ]] || fatal "$backup_dir/manifest.txt missing — is this a backup.sh output?"
[[ -f "$backup_dir/postgres.dump" ]] || fatal "$backup_dir/postgres.dump missing"
[[ -f "$backup_dir/backend.env" ]] || fatal "$backup_dir/backend.env missing"

log "mode:        $mode"
log "backup dir:  $backup_dir"

# ── 1. Manifest verification (all modes do this) ─────────────────────
log "verifying manifest checksums"
verify_failed=0
while IFS= read -r line; do
    [[ "$line" =~ ^[[:space:]]*$ ]] && continue
    [[ "$line" =~ ^── ]] && continue
    [[ "$line" =~ ^(Date|Generated|Host|Repo|Kway|postgres\.dump:|sparseimages:|WARNING) ]] && continue
    rel=$(echo "$line"  | awk '{print $1}')
    sha=$(echo "$line" | awk '{print $NF}')
    [[ -z "$rel" || -z "$sha" ]] && continue
    f="$backup_dir/$rel"
    if [[ ! -f "$f" ]]; then
        log "  MISSING: $rel"
        verify_failed=1
        continue
    fi
    actual=$(shasum -a 256 "$f" | awk '{print $1}')
    if [[ "$actual" != "$sha" ]]; then
        log "  CORRUPT: $rel (expected $sha, got $actual)"
        verify_failed=1
    fi
done < "$backup_dir/manifest.txt"

if (( verify_failed )); then
    fatal "manifest verification failed — refuse to restore"
fi
log "manifest OK — all files present and matching sha256"

if [[ "$mode" == check ]]; then
    log "check complete (no changes made)"
    exit 0
fi

# ── 2. Plan output ───────────────────────────────────────────────────
log ""
log "plan:"
log "  - stop backend + web launchd agents"
log "  - detach any mounted sparseimage under ${PROJECT_DATA_ROOT}/users/"
log "  - save current backend/.env -> backend/.env.pre-restore-<ts>"
log "  - save current DMG store    -> <dmg-root>.pre-restore-<ts>/"
log "  - DROP DATABASE kway_dev and pg_restore from $backup_dir/postgres.dump"
log "  - copy $backup_dir/sparseimages/*  -> $DMG_ROOT/"
log "  - restart backend"

if [[ "$mode" == dry ]]; then
    log ""
    log "dry-run complete (no changes made)"
    exit 0
fi

# ── 3. Apply ─────────────────────────────────────────────────────────
log ""
log "*** APPLYING RESTORE — this is destructive ***"
read -r -p "type 'RESTORE' to continue: " confirm
[[ "$confirm" == "RESTORE" ]] || fatal "aborted (not confirmed)"

ts=$(date +%Y%m%d-%H%M%S)

# 3a. Stop launchd agents
for label in com.kway.dev.backend com.kway.dev.web; do
    if launchctl print "gui/$(id -u)/$label" >/dev/null 2>&1; then
        log "stopping $label"
        launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
    fi
done

# 3b. Detach any mounted sparseimage
while IFS= read -r mp; do
    [[ -z "$mp" ]] && continue
    log "detaching $mp"
    hdiutil detach "$mp" 2>/dev/null || hdiutil detach -force "$mp" || true
done < <(/sbin/mount | awk -v root="${PROJECT_DATA_ROOT}/users/" '
    { for (i=1;i<=NF;i++) if ($i=="on") { print $(i+1); break } }
' | grep "^${PROJECT_DATA_ROOT}/users/" || true)

# 3c. Save current state
cp -p "${REPO_ROOT}/backend/.env" "${REPO_ROOT}/backend/.env.pre-restore-${ts}"
log "saved current backend/.env -> .env.pre-restore-${ts}"

if [[ -d "$DMG_ROOT" ]]; then
    mv "$DMG_ROOT" "${DMG_ROOT}.pre-restore-${ts}"
    log "saved current DMG store -> ${DMG_ROOT}.pre-restore-${ts}"
fi
mkdir -p "$DMG_ROOT"
chmod 700 "$DMG_ROOT"

# 3d. Restore backend/.env first (so backend can decrypt restored vault data)
cp -p "$backup_dir/backend.env" "${REPO_ROOT}/backend/.env"
chmod 600 "${REPO_ROOT}/backend/.env"
log "restored backend/.env"

# 3e. DB
log "DROP + recreate kway_dev"
docker compose -f "${REPO_ROOT}/docker-compose.yml" exec -T postgres \
    psql -U postgres -d postgres -c "DROP DATABASE IF EXISTS kway_dev;"
docker compose -f "${REPO_ROOT}/docker-compose.yml" exec -T postgres \
    psql -U postgres -d postgres -c "CREATE DATABASE kway_dev;"

log "pg_restore from postgres.dump"
docker compose -f "${REPO_ROOT}/docker-compose.yml" exec -T postgres \
    pg_restore -U postgres -d kway_dev --no-owner --no-privileges \
    < "$backup_dir/postgres.dump"

# 3f. Sparseimages
log "restoring sparseimages"
copied=0
if [[ -d "$backup_dir/sparseimages" ]]; then
    for img in "$backup_dir"/sparseimages/*.sparseimage; do
        [[ -e "$img" ]] || continue
        rsync -a --sparse "$img" "${DMG_ROOT}/"
        (( copied+=1 ))
    done
fi
log "restored $copied sparseimage(s)"

# 3g. Restart backend
log "restarting backend"
"${REPO_ROOT}/scripts/install-launchd.sh" install >/dev/null 2>&1 || true
sleep 3

log ""
log "RESTORE COMPLETE"
log "  current state was preserved as:"
log "    ${REPO_ROOT}/backend/.env.pre-restore-${ts}"
log "    ${DMG_ROOT}.pre-restore-${ts}"
log "  test by logging in as one of the restored users via the web UI."
log "  if everything works, you can rm those .pre-restore-* paths."
