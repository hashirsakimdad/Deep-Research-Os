# Starts a portable PostgreSQL (no Docker/WSL needed) on port 5433 and creates the `research` database.
# One-time setup: download the Windows binaries zip from https://www.enterprisedb.com/download-postgresql-binaries,
# unzip so that <PgRoot>\pgsql\bin\initdb.exe exists. Defaults to D:\postgres.
param([string]$PgRoot = "D:\postgres", [int]$Port = 5433)

$bin = Join-Path $PgRoot "pgsql\bin"
$data = Join-Path $PgRoot "data"
$env:PGPASSWORD = "research"

if (-not (Test-Path (Join-Path $bin "initdb.exe"))) { throw "PostgreSQL binaries not found in $bin" }

if (-not (Test-Path $data)) {
    $pw = Join-Path $PgRoot "pw.txt"
    Set-Content $pw "research" -Encoding ascii
    & "$bin\initdb.exe" -D $data -U research --pwfile=$pw -A scram-sha-256 -E UTF8 | Out-Null
    Remove-Item $pw
}

& "$bin\pg_isready.exe" -h localhost -p $Port | Out-Null
if ($LASTEXITCODE -ne 0) {
    Start-Process "$bin\pg_ctl.exe" -ArgumentList '-D', $data, '-l', (Join-Path $PgRoot "pg.log"), '-o', "`"-p $Port`"", 'start' -WindowStyle Hidden
    do { Start-Sleep 1; & "$bin\pg_isready.exe" -h localhost -p $Port | Out-Null } until ($LASTEXITCODE -eq 0)
}

$exists = & "$bin\psql.exe" -U research -h localhost -p $Port -d postgres -tAc "select 1 from pg_database where datname='research'"
if (-not $exists) { & "$bin\createdb.exe" -U research -h localhost -p $Port research }

Write-Host "PostgreSQL ready: postgres://research:research@localhost:$Port/research"
