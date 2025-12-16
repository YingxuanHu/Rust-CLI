#!/usr/bin/env bash
set -euo pipefail

MODEL="${1:-${MODEL:-llama3}}"
EMBED_MODEL="${EMBED_MODEL:-${LLM_CLI_EMBEDDING_MODEL:-nomic-embed-text}}"
CLASSIFIER_MODEL="${CLASSIFIER_MODEL:-${LLM_CLI_CLASSIFIER_MODEL:-qwen2:1.5b}}"

log() {
  printf '==> %s\n' "$*"
}

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' is not installed or in PATH" >&2
    exit 1
  fi
}

check_model() {
  local label="$1"
  local requested="$2"
  local bare="${requested%%:*}"
  local found=0

  while IFS= read -r name; do
    local base="${name%%:*}"
    if [ "$name" = "$requested" ] || [ "$base" = "$bare" ]; then
      found=1
      break
    fi
  done <<<"$available_models"

  if [ "$found" -ne 1 ]; then
    cat <<EOF
warn: ${label} model "$requested" not found locally.
Pull it with: ollama pull "$requested"
EOF
  else
    log "$label model $requested is available (matched $bare)"
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

available_models=$(ollama list | awk 'NR>1 {print $1}')

check_model "chat" "$MODEL"
check_model "embedding" "$EMBED_MODEL"
check_model "classifier" "$CLASSIFIER_MODEL"

log "Health check completed"
