# reindex-all.ps1 - re-index every git/upload project in one shot.
#
# Why: Phase 3 embedder v2 (fastembed multilingual) changed the vector
# dimension (256 -> 384). Old vectors no longer match, so each project
# must be re-indexed once to populate the new cross-lingual vectors.
# This script logs in, lists projects, and calls the re-index endpoint
# for every git/upload project (local projects have nothing to index).
#
# ASCII-only on purpose: Windows PowerShell 5.1 parses .ps1 with the
# system ANSI codepage; non-ASCII without a BOM corrupts the file.
# Keep this script English-only (runtime data from the API may be
# non-ASCII - that is fine, only the script bytes must stay ASCII).
#
# Login is the Kway Dev APP account (kway_dev DB users table), NOT any
# other email. No default account is baked in - you must supply it.
#
# Usage:
#   .\reindex-all.ps1                         prompt for email + password
#   .\reindex-all.ps1 -Email a@b -Password p  non-interactive
#   $env:KWAY_EMAIL='a@b'; $env:KWAY_PASSWORD='p'; .\reindex-all.ps1
#
# Backend must be running (http://localhost:8888). Start it first with
# .\start-all.ps1 if needed.

param(
    [string]$BaseUrl  = "http://localhost:8888/api",
    [string]$Email    = $env:KWAY_EMAIL,
    [string]$Password = $env:KWAY_PASSWORD
)

$ErrorActionPreference = "Stop"

# ---- email -----------------------------------------------------------
# No baked-in default on purpose: this logs into the Kway Dev APP
# account system (the kway_dev DB users table), which is unrelated to
# any other email. Supply -Email, set $env:KWAY_EMAIL, or enter it when
# prompted.
if ([string]::IsNullOrWhiteSpace($Email)) {
    $Email = Read-Host "Kway Dev login email"
}

# ---- password --------------------------------------------------------
if ([string]::IsNullOrWhiteSpace($Password)) {
    $secure = Read-Host "Password for $Email" -AsSecureString
    $bstr   = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    $Password = [Runtime.InteropServices.Marshal]::PtrToStringAuto($bstr)
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
}

# ---- 1. login --------------------------------------------------------
Write-Host "[auth] logging in as $Email ..." -ForegroundColor Cyan
try {
    $loginBody = @{ email = $Email; password = $Password } | ConvertTo-Json
    $login = Invoke-RestMethod -Method Post -Uri "$BaseUrl/auth/login" `
        -ContentType "application/json" -Body $loginBody -TimeoutSec 15
}
catch {
    Write-Host "[auth] login failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "       (is the backend up? wrong password? run .\start-all.ps1)" -ForegroundColor DarkGray
    return
}
$token = $login.access_token
if ([string]::IsNullOrWhiteSpace($token)) {
    Write-Host "[auth] no access_token in response - aborting" -ForegroundColor Red
    return
}
$headers = @{ Authorization = "Bearer $token" }
Write-Host "[auth] OK" -ForegroundColor Green

# ---- 2. list projects ------------------------------------------------
Write-Host "[list] fetching projects ..." -ForegroundColor Cyan
try {
    $projects = Invoke-RestMethod -Method Get -Uri "$BaseUrl/projects" `
        -Headers $headers -TimeoutSec 30
}
catch {
    Write-Host "[list] failed: $($_.Exception.Message)" -ForegroundColor Red
    return
}

$targets = @($projects | Where-Object { $_.source_type -in @("git", "upload") })
$skipped = @($projects | Where-Object { $_.source_type -notin @("git", "upload") })

Write-Host ("[list] {0} project(s); {1} indexable (git/upload), {2} skipped (local)" -f `
    $projects.Count, $targets.Count, $skipped.Count) -ForegroundColor Green
if ($targets.Count -eq 0) {
    Write-Host "[done] nothing to re-index." -ForegroundColor Yellow
    return
}

# ---- 3. re-index each (continue on per-project failure) --------------
$ok = 0; $fail = 0
foreach ($p in $targets) {
    Write-Host ("[idx ] {0} ({1}) ..." -f $p.name, $p.source_type) `
        -ForegroundColor Cyan -NoNewline
    try {
        $r = Invoke-RestMethod -Method Post -Uri "$BaseUrl/projects/$($p.id)/index" `
            -Headers $headers -TimeoutSec 600
        Write-Host (" {0} files" -f $r.indexed_files) -ForegroundColor Green
        $ok++
    }
    catch {
        Write-Host (" FAILED: {0}" -f $_.Exception.Message) -ForegroundColor Red
        $fail++
    }
}

# ---- summary ---------------------------------------------------------
Write-Host ""
Write-Host ("[done] re-indexed {0} ok, {1} failed, {2} local skipped." -f `
    $ok, $fail, $skipped.Count) -ForegroundColor Green
Write-Host "Verify vectors landed (psql):" -ForegroundColor DarkGray
Write-Host "  SELECT count(*) total, count(embedding) with_vec FROM project_file_chunks;" -ForegroundColor DarkGray
Write-Host "First re-index is slower (every chunk goes through the ONNX model)." -ForegroundColor DarkGray
