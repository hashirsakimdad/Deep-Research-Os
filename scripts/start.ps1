# Starts everything for Deep Research OS: database, backend, frontend. Safe to run again (skips what is up).
#   ./scripts/start.ps1                 # frontend :3001, backend :8081 (avoids a common clash on :3000/:8080)
#   ./scripts/start.ps1 -FrontendPort 3000 -BackendPort 8080
param([int]$FrontendPort = 3001, [int]$BackendPort = 8081, [switch]$NoBrowser)

$root = Split-Path -Parent $PSScriptRoot
$cargoHome = "D:\cargo"; $targetDir = "D:\cargo-target\deep-research-os"   # keep builds off a small system drive
if (-not (Test-Path $cargoHome)) { $cargoHome = Join-Path $env:USERPROFILE ".cargo"; $targetDir = Join-Path $root "backend\target" }
$logs = Join-Path $root ".logs"; New-Item -ItemType Directory -Force $logs | Out-Null

function Test-Port($p) { (Test-NetConnection localhost -Port $p -WarningAction SilentlyContinue).TcpTestSucceeded }
function Wait-Http($url, $tries = 60) {
    for ($i = 0; $i -lt $tries; $i++) { try { Invoke-WebRequest $url -UseBasicParsing -TimeoutSec 5 | Out-Null; return $true } catch { Start-Sleep 3 } }
    $false
}

if (-not (Test-Path (Join-Path $root ".env"))) { throw ".env missing: copy .env.example to .env and set GROK_API_KEY and SEARCH_API_KEY" }

# 1. database (portable Postgres on :5433; skip if DATABASE_URL points elsewhere and is reachable)
Write-Host "[1/3] database"
& (Join-Path $PSScriptRoot "dev-db.ps1")

# 2. backend
Write-Host "[2/3] backend on :$BackendPort"
if (-not (Test-Port $BackendPort)) {
    $env:BIND_ADDR = "0.0.0.0:$BackendPort"; $env:CORS_ORIGIN = "http://localhost:$FrontendPort"
    $env:CARGO_HOME = $cargoHome; $env:CARGO_TARGET_DIR = $targetDir; $env:CARGO_BUILD_JOBS = "1"; $env:CARGO_INCREMENTAL = "0"
    Push-Location (Join-Path $root "backend")
    Start-Process cargo -ArgumentList "run" -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "backend.log") -RedirectStandardError (Join-Path $logs "backend.err")
    Pop-Location
}
if (-not (Wait-Http "http://localhost:$BackendPort/health" 120)) { throw "backend did not come up; see .logs\backend.err" }

# 3. frontend (webpack mode is lighter on memory than turbopack)
Write-Host "[3/3] frontend on :$FrontendPort"
if (-not (Test-Port $FrontendPort)) {
    $env:NEXT_PUBLIC_API_URL = "http://localhost:$BackendPort"
    Push-Location (Join-Path $root "frontend")
    if (-not (Test-Path node_modules)) { npm install | Out-Null }
    if (Test-Path "D:\tmp") { $env:TEMP = "D:\tmp"; $env:TMP = "D:\tmp" }   # keep temp files off a small system drive
    Start-Process npx.cmd -ArgumentList "next", "dev", "--webpack", "-p", $FrontendPort -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "frontend.log") -RedirectStandardError (Join-Path $logs "frontend.err")
    Pop-Location
}
if (-not (Wait-Http "http://localhost:$FrontendPort" 90)) { throw "frontend did not come up; see .logs\frontend.err" }

Write-Host "`nDeep Research OS is running:  http://localhost:$FrontendPort   (API http://localhost:$BackendPort)"
if (-not $NoBrowser) { Start-Process "http://localhost:$FrontendPort" }
