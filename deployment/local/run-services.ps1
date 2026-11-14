# Runs the whole Red Hive backend on Windows without Docker (development).
# Needs PostgreSQL with deployment/docker/database/*.sql applied and `cargo build`.
#   $env:DATABASE_URL = "postgres://redhive:redhive@localhost:5432/redhive"; .\deployment\local\run-services.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$bin  = if ($env:BIN) { $env:BIN } else { "target\debug" }
$logs = if ($env:LOGS) { $env:LOGS } else { Join-Path $env:TEMP "redhive-logs" }
New-Item -ItemType Directory -Force $logs | Out-Null
if (-not $env:DATABASE_URL) { $env:DATABASE_URL = "postgres://redhive:redhive@localhost:5432/redhive" }
if (-not $env:REDHIVE__MODULES_DIR) { $env:REDHIVE__MODULES_DIR = "modules" }
$services = [ordered]@{
  "identity-service"      = @(8101, "redhive-identity-service")
  "configuration-service" = @(8102, "redhive-configuration-service")
  "module-registry"       = @(8103, "redhive-module-registry-service")
  "module-delivery"       = @(8104, "redhive-module-delivery-service")
  "update-orchestrator"   = @(8105, "redhive-update-orchestrator")
  "notification-point"    = @(8106, "redhive-notification-point")
  "workflow-service"      = @(8107, "redhive-workflow-service")
  "ai-agent"              = @(8108, "redhive-ai-agent-service")
  "document-service"      = @(8111, "redhive-document-service")
  "reporting-service"     = @(8112, "redhive-reporting-service")
  "user-service"          = @(8113, "redhive-user-service")
  "api-gateway"           = @(8080, "redhive-api-gateway")
}
foreach ($name in $services.Keys) {
  $var = "REDHIVE__SERVICE__" + ($name.ToUpper() -replace "-", "_")
  Set-Item "env:$var" "http://localhost:$($services[$name][0])"
}
$procs = @()
foreach ($name in $services.Keys) {
  $env:REDHIVE__PORT = $services[$name][0]
  $exe = Join-Path $bin "$($services[$name][1]).exe"
  $procs += Start-Process -FilePath $exe -NoNewWindow -PassThru `
    -RedirectStandardOutput (Join-Path $logs "$name.log") -RedirectStandardError (Join-Path $logs "$name.err.log")
  Write-Host "$name -> :$($services[$name][0]) (pid $($procs[-1].Id))"
}
Write-Host "Logs in $logs - press Ctrl+C to stop all services."
try { Wait-Process -Id ($procs | ForEach-Object Id) } finally { $procs | Stop-Process -ErrorAction SilentlyContinue }
