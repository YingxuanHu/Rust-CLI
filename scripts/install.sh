#!/usr/bin/env bash
# Install llm_cli from a checked-out copy of this repository.
#
# Usage:
#   bash scripts/install.sh
#   bash scripts/install.sh --with-models

set -euo pipefail

install_models=false
if [ "${1:-}" = "--with-models" ]; then
  install_models=true
elif [ "$#" -ne 0 ]; then
  echo "usage: bash scripts/install.sh [--with-models]" >&2
  exit 2
fi

# Keep optional model pulls pointed at the same daemon that llm_cli will use.
if [ -n "${LLM_CLI_OLLAMA_HOST:-}" ]; then
  export OLLAMA_HOST="$LLM_CLI_OLLAMA_HOST"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: Rust/Cargo is required. Install it with rustup: https://rustup.rs" >&2
  exit 1
fi

project_dir="$(cd -- "$(dirname -- "$0")/.." && pwd)"
echo "Installing llm_cli from ${project_dir}"
cargo install --path "$project_dir" --locked

if ! command -v ollama >/dev/null 2>&1; then
  cat <<'EOF'

llm_cli is installed, but Ollama is not available yet.
Install Ollama from https://ollama.com, then pull the required models:
  ollama pull llama3
  ollama pull nomic-embed-text
  ollama pull qwen2:1.5b
EOF
  exit 0
fi

if [ "$install_models" = true ]; then
  ollama pull llama3
  ollama pull nomic-embed-text
  ollama pull qwen2:1.5b
fi

cat <<'EOF'

Next steps, from the repository you want to work in:
  llm_cli setup
  llm_cli doctor --full
  llm_cli

Pass --with-models to this installer to pull the default local models now.
EOF
