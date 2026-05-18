# start-all.ps1 — 一鍵啟動 Kway Dev 本地環境
#
# 用法：
#   .\start-all.ps1            # 直接啟動（用現有 backend binary）
#   .\start-all.ps1 -Build     # 先 cargo build --release 再啟動
#   .\start-all.ps1 -Stop      # 關掉 backend / frontend
#
# 三個服務：
#   1. PostgreSQL (Windows service postgresql-x64-18)
#   2. Rust backend  → http://localhost:8888  (cmd 包一層繞 WDAC)
#   3. Next.js front → http://localhost:3000
#
# Backend binary 在 C:\rust-build\kway-backend\release\ 不是專案 target/，
# 這是為了繞過 WDAC 封鎖設定的 CARGO_TARGET_DIR。

param(
    [switch]$Build,
    [switch]$Stop
)

$ErrorActionPreference = "Stop"
$ProjectRoot = "C:\Users\kicl1\OneDrive\文件\研發組專案\Kway_In-house_Dev_Automation_Tools"
$BackendDir  = Join-Path $ProjectRoot "backend"
$WebDir      = Join-Path $ProjectRoot "web"
$BackendExe  = "C:\rust-build\kway-backend\release\kway-dev-backend.exe"
$PgService   = "postgresql-x64-18"

# ── Stop mode ─────────────────────────────────────────────────────────
if ($Stop) {
    Write-Host "[stop] 關閉 backend / frontend..." -ForegroundColor Yellow
    Get-Process -Name "kway-dev-backend" -ErrorAction SilentlyContinue |
        Stop-Process -Force
    # next dev 跑在 node 底下；只關掉 cwd 在 web/ 的那個比較難判斷，
    # 這裡用 port 3000 來找。
    $p = (Get-NetTCPConnection -LocalPort 3000 -State Listen -ErrorAction SilentlyContinue).OwningProcess
    if ($p) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
    Write-Host "[stop] 完成（PostgreSQL service 保留不動）" -ForegroundColor Green
    return
}

# ── 1. PostgreSQL ─────────────────────────────────────────────────────
$svc = Get-Service $PgService -ErrorAction SilentlyContinue
if ($null -eq $svc) {
    Write-Host "[db]   找不到服務 $PgService — 請確認 PostgreSQL 18 已安裝" -ForegroundColor Red
} elseif ($svc.Status -ne "Running") {
    Write-Host "[db]   啟動 $PgService..." -ForegroundColor Cyan
    Start-Service $PgService
    Write-Host "[db]   PostgreSQL 已啟動" -ForegroundColor Green
} else {
    Write-Host "[db]   PostgreSQL 已在執行" -ForegroundColor Green
}

# ── 2. Backend ────────────────────────────────────────────────────────
if ($Build) {
    Write-Host "[be]   cargo build --release（改過 code 才需要，稍等...）" -ForegroundColor Cyan
    Push-Location $BackendDir
    cargo build --release
    Pop-Location
    Write-Host "[be]   編譯完成" -ForegroundColor Green
}

if (-not (Test-Path $BackendExe)) {
    Write-Host "[be]   找不到 $BackendExe — 先跑一次 .\start-all.ps1 -Build" -ForegroundColor Red
    return
}

# 已經在跑就先關掉舊的，避免 port 8888 卡住
Get-Process -Name "kway-dev-backend" -ErrorAction SilentlyContinue |
    Stop-Process -Force
Start-Sleep -Milliseconds 500

Write-Host "[be]   啟動 backend（新視窗，cmd 繞 WDAC）..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList @(
    "-NoExit", "-Command",
    "Set-Location '$BackendDir'; cmd /c `"$BackendExe`""
)

# ── 3. Frontend ───────────────────────────────────────────────────────
Write-Host "[fe]   啟動 Next.js dev（新視窗）..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList @(
    "-NoExit", "-Command",
    "Set-Location '$WebDir'; npm run dev"
)

# ── 健康檢查 ──────────────────────────────────────────────────────────
Write-Host ""
Write-Host "[wait] 等 backend 起來（最多 60 秒）..." -ForegroundColor Cyan
$ok = $false
for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 1
    try {
        $r = Invoke-WebRequest -Uri "http://localhost:8888/healthz" `
             -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        if ($r.StatusCode -eq 200) { $ok = $true; break }
    } catch { }
}

Write-Host ""
if ($ok) {
    Write-Host "✓ Backend  http://localhost:8888  (healthz OK)" -ForegroundColor Green
} else {
    Write-Host "✗ Backend 60 秒內沒回應 — 看新開的 backend 視窗錯誤訊息" -ForegroundColor Red
}
Write-Host "→ Frontend http://localhost:3000  (Next.js 首次編譯需 10-30 秒)" -ForegroundColor Green
Write-Host ""
Write-Host "關閉：.\start-all.ps1 -Stop" -ForegroundColor DarkGray
