# start-all.ps1 - one-shot launcher for the Kway Dev local environment.
#
# ASCII-only on purpose: Windows PowerShell 5.1 parses .ps1 with the
# system ANSI codepage, so non-ASCII without a BOM corrupts the file and
# throws "MissingArrayIndexExpression". Keep this script English-only.
#
# Usage:
#   .\start-all.ps1            start (use existing backend binary)
#   .\start-all.ps1 -Build     cargo build --release first, then start
#   .\start-all.ps1 -Stop      stop backend + frontend
#
# Services:
#   1. PostgreSQL  (Windows service postgresql-x64-18)
#   2. Rust backend -> http://localhost:8888  (launched via cmd, WDAC)
#   3. Next.js dev  -> http://localhost:3000
#
# Backend binary lives at C:\rust-build\kway-backend\release\ (the
# CARGO_TARGET_DIR override used to dodge WDAC), not the project target/.

param(
    [switch]$Build,
    [switch]$Stop
)

$ErrorActionPreference = "Stop"
# Derive the project root from the script's own location so this file
# stays 100% ASCII (the repo path contains non-ASCII characters; hard-
# coding it would reintroduce the codepage corruption this rewrite fixes).
$ProjectRoot = $PSScriptRoot
$BackendDir  = Join-Path $ProjectRoot "backend"
$WebDir      = Join-Path $ProjectRoot "web"
$BackendExe  = "C:\rust-build\kway-backend\release\kway-dev-backend.exe"
$PgService   = "postgresql-x64-18"

# ---- Stop mode -------------------------------------------------------
if ($Stop) {
    Write-Host "[stop] stopping backend / frontend..." -ForegroundColor Yellow
    Get-Process -Name "kway-dev-backend" -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
    $conn = Get-NetTCPConnection -LocalPort 3000 -State Listen -ErrorAction SilentlyContinue
    if ($conn) {
        Stop-Process -Id $conn.OwningProcess -Force -ErrorAction SilentlyContinue
    }
    Write-Host "[stop] done (PostgreSQL service left running)" -ForegroundColor Green
    return
}

# ---- 1. PostgreSQL ---------------------------------------------------
$svc = Get-Service $PgService -ErrorAction SilentlyContinue
if ($null -eq $svc) {
    Write-Host "[db] service $PgService not found - is PostgreSQL 18 installed?" -ForegroundColor Red
}
elseif ($svc.Status -ne "Running") {
    Write-Host "[db] starting $PgService..." -ForegroundColor Cyan
    Start-Service $PgService
    Write-Host "[db] PostgreSQL started" -ForegroundColor Green
}
else {
    Write-Host "[db] PostgreSQL already running" -ForegroundColor Green
}

# ---- 2. Backend ------------------------------------------------------
if ($Build) {
    Write-Host "[be] cargo build --release (only needed after code changes)..." -ForegroundColor Cyan
    Push-Location $BackendDir
    cargo build --release
    Pop-Location
    Write-Host "[be] build complete" -ForegroundColor Green
}

if (-not (Test-Path $BackendExe)) {
    Write-Host "[be] $BackendExe missing - run .\start-all.ps1 -Build first" -ForegroundColor Red
    return
}

# Kill any old instance so port 8888 is free.
Get-Process -Name "kway-dev-backend" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

Write-Host "[be] launching backend (new window, cmd wrapper for WDAC)..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList @(
    "-NoExit", "-Command",
    "Set-Location '$BackendDir'; cmd /c `"$BackendExe`""
)

# ---- 3. Frontend -----------------------------------------------------
Write-Host "[fe] launching Next.js dev (new window)..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList @(
    "-NoExit", "-Command",
    "Set-Location '$WebDir'; npm run dev"
)

# ---- health check ----------------------------------------------------
Write-Host ""
Write-Host "[wait] waiting up to 60s for backend healthz..." -ForegroundColor Cyan
$ok = $false
for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 1
    try {
        $r = Invoke-WebRequest -Uri "http://localhost:8888/healthz" `
             -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        if ($r.StatusCode -eq 200) { $ok = $true; break }
    }
    catch { }
}

Write-Host ""
if ($ok) {
    Write-Host "OK  Backend  http://localhost:8888  (healthz 200)" -ForegroundColor Green
}
else {
    Write-Host "XX  Backend not responding in 60s - check the new backend window" -ForegroundColor Red
}
Write-Host "->  Frontend http://localhost:3000  (first Next.js compile 10-30s)" -ForegroundColor Green
Write-Host ""
Write-Host "Stop with: .\start-all.ps1 -Stop" -ForegroundColor DarkGray
