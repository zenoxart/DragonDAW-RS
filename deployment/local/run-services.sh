#!/usr/bin/env bash
# Runs the whole Red Hive backend without Docker (development). Needs PostgreSQL with the schema from
# deployment/docker/database and a release or debug build (`cargo build`).
#   DATABASE_URL=postgres://redhive:redhive@localhost:5432/redhive ./deployment/local/run-services.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN=${BIN:-target/debug}
LOGS=${LOGS:-/tmp/redhive-logs}
mkdir -p "$LOGS"
export DATABASE_URL=${DATABASE_URL:-postgres://redhive:redhive@localhost:5432/redhive}
export REDHIVE__MODULES_DIR=${REDHIVE__MODULES_DIR:-modules}
declare -A PORTS=(
  [identity-service]=8101 [configuration-service]=8102 [module-registry]=8103 [module-delivery]=8104
  [update-orchestrator]=8105 [notification-point]=8106 [workflow-service]=8107 [ai-agent]=8108
  [document-service]=8111 [reporting-service]=8112 [user-service]=8113 [api-gateway]=${REDHIVE_PORT:-8080}
)
declare -A BINS=(
  [identity-service]=redhive-identity-service [configuration-service]=redhive-configuration-service
  [module-registry]=redhive-module-registry-service [module-delivery]=redhive-module-delivery-service
  [update-orchestrator]=redhive-update-orchestrator [notification-point]=redhive-notification-point
  [workflow-service]=redhive-workflow-service [ai-agent]=redhive-ai-agent-service
  [document-service]=redhive-document-service [reporting-service]=redhive-reporting-service
  [user-service]=redhive-user-service [api-gateway]=redhive-api-gateway
)
for name in "${!PORTS[@]}"; do
  var="REDHIVE__SERVICE__$(echo "$name" | tr 'a-z-' 'A-Z_')"
  export "$var=http://localhost:${PORTS[$name]}"
done
for name in "${!BINS[@]}"; do
  REDHIVE__PORT=${PORTS[$name]} "$BIN/${BINS[$name]}" > "$LOGS/$name.log" 2>&1 &
  echo "$name → :${PORTS[$name]} (pid $!)"
done
echo "Logs in $LOGS – stop with: pkill -f 'redhive-'"
wait
