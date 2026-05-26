#!/usr/bin/env bash
# Kway Dev — nightly backup.
#
# Captures three things, each of which on its own is enough to brick
# every existing user's vault if lost:
#
#   1. Postgres dump (users, projects, vault_secrets, vault_key_wrappings,
#      vault_ciphertexts, organization_members, …)
#   2. backend/.env (JWT_SECRET + GIT_TOKEN_ENCRYPTION_KEY — losing
#      these orphans every vault entry in the DB above)
#   3. ~/kway-dmg-store/*.sparseimage (encrypted per-user volumes;
#      AES-encrypted on disk but useless without the system + user
#      KEK wrappings in the DB above)
#
# Output layout:
#   <BACKUP_ROOT>/<YYYY-MM-DD>/
#       postgres.dump          pg_dump custom format (.dump → pg_restore)
#       backend.env            verbatim copy
#       sparseimages/<uuid>.sparseimage
#       manifest.txt           file sizes + sha256 + timestamps
#       backup.log             full stdout/stderr of this run
#
# Retention:
#   • daily   — last 7 dirs
#   • weekly  — first daily of each ISO week, last 4
#   • monthly — first daily of each calendar month, last 12
#
# Idempotent: running twice on the same day overwrites that day's
# directory atomically (writes to .tmp, swaps at end).
#
# Skipped if any sparseimage is currently mounted (a user is logged in)
# and SKIP_MOUNTED=1 (the default). To force-backup with potentially
# inconsistent volumes set SKIP_MOUNTED=0; callers should logout users
# first.

set -euo pipefail

# ── Config knobs ─────────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BACKUP_ROOT="${BACKUP_ROOT:-${HOME}/kway-backups}"
DMG_ROOT="${DMG_ROOT:-${HOME}/kway-dmg-store}"
PROJECT_DATA_ROOT="${PROJECT_DATA_ROOT:-${HOME}/kway-project-data}"
SKIP_MOUNTED="${SKIP_MOUNTED:-1}"
RETAIN_DAILY="${RETAIN_DAILY:-7}"
RETAIN_WEEKLY="${RETAIN_WEEKLY:-4}"
RETAIN_MONTHLY="${RETAIN_MONTHLY:-12}"

# ── Helpers ──────────────────────────────────────────────────────────
TODAY=$(date +%Y-%m-%d)
DAY_DIR="${BACKUP_ROOT}/${TODAY}"
TMP_DIR="${DAY_DIR}.tmp"

log() { printf "[%s] %s\n" "$(date +%Y-%m-%dT%H:%M:%S%z)" "$*"; }

fatal() { log "FATAL: $*"; exit 1; }

# Pretty bytes
human_bytes() {
    local b=$1
    if (( b < 1024 )); then printf "%dB" "$b"
    elif (( b < 1048576 )); then printf "%.1fK" "$(echo "$b/1024" | bc -l)"
    elif (( b < 1073741824 )); then printf "%.1fM" "$(echo "$b/1048576" | bc -l)"
    else printf "%.2fG" "$(echo "$b/1073741824" | bc -l)"
    fi
}

# ── Pre-flight ───────────────────────────────────────────────────────
log "backup root: ${BACKUP_ROOT}"
log "target day:  ${TODAY}"

if ! docker compose -f "${REPO_ROOT}/docker-compose.yml" ps postgres 2>/dev/null | grep -q "Up "; then
    fatal "postgres container not running — start it with: docker compose up -d postgres"
fi
log "postgres container OK"

if [[ ! -f "${REPO_ROOT}/backend/.env" ]]; then
    fatal "backend/.env not found — refusing to back up without it (would be useless)"
fi
log "backend/.env present"

# Mounted-sparseimage gate.
MOUNTED_UUIDS=()
while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    MOUNTED_UUIDS+=("$line")
done < <(/sbin/mount 2>/dev/null | awk -v root="${PROJECT_DATA_ROOT}/users/" '
    {
        # `/dev/diskNsM on <mount-point> (...)` — pull the mount point
        for (i = 1; i <= NF; i++) if ($i == "on") { mp = $(i+1); break }
        if (index(mp, root) == 1) {
            n = split(mp, parts, "/")
            print parts[n]
        }
    }
')

if (( ${#MOUNTED_UUIDS[@]} > 0 )); then
    log "WARNING: ${#MOUNTED_UUIDS[@]} sparseimage(s) currently mounted: ${MOUNTED_UUIDS[*]}"
    if [[ "$SKIP_MOUNTED" == "1" ]]; then
        log "    -> will skip those (SKIP_MOUNTED=1 default). DB dump still happens."
    else
        log "    -> SKIP_MOUNTED=0 — backing up anyway. Volume contents may be inconsistent."
    fi
fi

# ── Build target dir ─────────────────────────────────────────────────
mkdir -p "${BACKUP_ROOT}"
rm -rf "${TMP_DIR}"
mkdir -p "${TMP_DIR}/sparseimages"

# Stream all log() output also to backup.log
exec > >(tee -a "${TMP_DIR}/backup.log") 2>&1

# ── 1. Postgres dump ─────────────────────────────────────────────────
log "step 1/3: pg_dump"
docker compose -f "${REPO_ROOT}/docker-compose.yml" exec -T postgres \
    pg_dump -U postgres -Fc -Z6 kway_dev > "${TMP_DIR}/postgres.dump"
PG_BYTES=$(stat -f%z "${TMP_DIR}/postgres.dump")
log "    wrote postgres.dump  ($(human_bytes "$PG_BYTES"))"

# ── 2. backend/.env ──────────────────────────────────────────────────
log "step 2/3: backend.env"
cp -p "${REPO_ROOT}/backend/.env" "${TMP_DIR}/backend.env"
chmod 600 "${TMP_DIR}/backend.env"
log "    copied backend.env  (chmod 600)"

# ── 3. Sparseimages ──────────────────────────────────────────────────
log "step 3/3: sparseimages"
sparse_count=0
sparse_skipped=0
if [[ -d "${DMG_ROOT}" ]]; then
    for img in "${DMG_ROOT}"/*.sparseimage; do
        [[ -e "$img" ]] || continue
        uuid=$(basename "$img" .sparseimage)
        if [[ "$SKIP_MOUNTED" == "1" ]] && printf "%s\n" "${MOUNTED_UUIDS[@]}" | grep -qx "$uuid"; then
            log "    skip ${uuid} (mounted)"
            (( sparse_skipped+=1 ))
            continue
        fi
        # rsync with --sparse keeps sparseimages efficient (only allocated
        # bytes are read/written), --partial allows resume if interrupted.
        rsync -a --sparse --partial "$img" "${TMP_DIR}/sparseimages/"
        bytes=$(stat -f%z "${TMP_DIR}/sparseimages/$(basename "$img")")
        log "    copied ${uuid}  ($(human_bytes "$bytes"))"
        (( sparse_count+=1 ))
    done
fi
log "    sparseimages: ${sparse_count} backed up, ${sparse_skipped} skipped"

# ── Manifest ─────────────────────────────────────────────────────────
log "writing manifest"
{
    echo "Kway Dev backup manifest"
    echo "Date: ${TODAY}"
    echo "Generated: $(date -Iseconds)"
    echo "Host: $(hostname)"
    echo "Repo: ${REPO_ROOT}"
    echo
    echo "── Files ──"
    while IFS= read -r f; do
        rel="${f#${TMP_DIR}/}"
        [[ "$rel" == "manifest.txt" ]] && continue
        [[ "$rel" == "backup.log" ]] && continue
        size=$(stat -f%z "$f")
        sha=$(shasum -a 256 "$f" | awk '{print $1}')
        printf "%-50s  %14s  %s\n" "$rel" "$size" "$sha"
    done < <(find "${TMP_DIR}" -type f | sort)
    echo
    echo "── Summary ──"
    echo "postgres.dump:    $(human_bytes "$PG_BYTES")"
    echo "sparseimages:     ${sparse_count} backed up, ${sparse_skipped} skipped"
    if (( sparse_skipped > 0 )); then
        echo "WARNING: ${sparse_skipped} sparseimage(s) skipped due to mount —"
        echo "  back up again after those users log out, or set SKIP_MOUNTED=0."
    fi
} > "${TMP_DIR}/manifest.txt"

# ── Atomic swap ──────────────────────────────────────────────────────
if [[ -d "${DAY_DIR}" ]]; then
    log "replacing existing ${TODAY} backup"
    rm -rf "${DAY_DIR}.old"
    mv "${DAY_DIR}" "${DAY_DIR}.old"
fi
mv "${TMP_DIR}" "${DAY_DIR}"
rm -rf "${DAY_DIR}.old"

TOTAL_BYTES=$(du -sk "${DAY_DIR}" | awk '{print $1*1024}')
log "backup complete: ${DAY_DIR}  ($(human_bytes "$TOTAL_BYTES") total)"

# ── Retention pruning ────────────────────────────────────────────────
log "applying retention policy (daily=${RETAIN_DAILY}, weekly=${RETAIN_WEEKLY}, monthly=${RETAIN_MONTHLY})"

# Collect all daily backup dirs, sorted oldest → newest.
all_dirs=()
while IFS= read -r d; do
    all_dirs+=("$d")
done < <(find "${BACKUP_ROOT}" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' | sort)

# Identify dirs to KEEP (union of three policies).
keep=()
# Last N daily
n=${#all_dirs[@]}
start=$(( n > RETAIN_DAILY ? n - RETAIN_DAILY : 0 ))
for (( i=start; i<n; i++ )); do
    keep+=("${all_dirs[i]}")
done
# First-of-week (Monday) — last RETAIN_WEEKLY
seen_weeks=()
for (( i=n-1; i>=0; i-- )); do
    d=$(basename "${all_dirs[i]}")
    wk=$(date -j -f "%Y-%m-%d" "$d" "+%G-W%V" 2>/dev/null) || continue
    if ! printf "%s\n" "${seen_weeks[@]}" 2>/dev/null | grep -qx "$wk"; then
        seen_weeks+=("$wk")
        keep+=("${all_dirs[i]}")
        (( ${#seen_weeks[@]} >= RETAIN_WEEKLY )) && break
    fi
done
# First-of-month — last RETAIN_MONTHLY
seen_months=()
for (( i=n-1; i>=0; i-- )); do
    d=$(basename "${all_dirs[i]}")
    mo=${d%-*}     # YYYY-MM
    if ! printf "%s\n" "${seen_months[@]}" 2>/dev/null | grep -qx "$mo"; then
        seen_months+=("$mo")
        keep+=("${all_dirs[i]}")
        (( ${#seen_months[@]} >= RETAIN_MONTHLY )) && break
    fi
done

# Anything not in keep → delete.
for d in "${all_dirs[@]}"; do
    if ! printf "%s\n" "${keep[@]}" | grep -qx "$d"; then
        log "    pruning $(basename "$d")"
        rm -rf "$d"
    fi
done

log "done."
