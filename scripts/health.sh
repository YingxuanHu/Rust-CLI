#!/usr/bin/env bash
set -euo pipefail

MODEL="${1:-${MODEL:-llama3}}"
MODEL_BARE="${MODEL%%:*}"

log() {
  printf '==> %s\n' "$*"
}

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' is not installed or in PATH" >&2
    exit 1
  fi
}

log "Checking toolchain"
require rustc
require cargo
require ollama
log "rustc $(rustc --version)"
log "cargo $(cargo --version)"
log "ollama $(ollama --version)"

log "Checking Ollama daemon"
if ! ollama list >/dev/null 2>&1; then
  cat <<'EOF' >&2
error: could not reach the Ollama daemon.
Start it with: ollama serve
EOF
  exit 1
fi

log "Validating model availability ($MODEL)"
available_models=$(ollama list | awk 'NR>1 {print $1}')
found=0
while IFS= read -r name; do
  base="${name%%:*}"
  if [ "$name" = "$MODEL" ] || [ "$base" = "$MODEL_BARE" ]; then
    found=1
    break
  fi
done <<<"$available_models"

if [ "$found" -ne 1 ]; then
  cat <<EOF
warn: model "$MODEL" not found locally.
Pull it with: ollama pull "$MODEL"
EOF
else
  log "Model $MODEL is available (matched $MODEL_BARE)"
fi

log "Health check completed"
