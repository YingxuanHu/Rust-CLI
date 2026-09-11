#!/usr/bin/env bash
# Install llm_cli from a checked-out copy or a verified GitHub release.
#
# Usage:
#   bash scripts/install.sh
#   bash scripts/install.sh --with-models
#   bash scripts/install.sh --binary [--version v0.1.0] [--with-models]

set -euo pipefail

install_models=false
install_binary=false
release_version="latest"
install_dir="${LLM_CLI_INSTALL_DIR:-$HOME/.local/bin}"
install_dir_requested=false

usage() {
  cat <<'EOF'
usage: bash scripts/install.sh [--binary] [--with-models] [--version vX.Y.Z] [--install-dir DIRECTORY]

Without --binary, build and install from this checked-out source tree with Cargo.
With --binary, download a release archive, verify it against the published
SHA256SUMS asset, and install it without requiring Cargo.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --with-models)
      install_models=true
      ;;
    --binary)
      install_binary=true
      ;;
    --version)
      shift
      if [ "$#" -eq 0 ] || [[ ! "$1" =~ ^v[0-9A-Za-z._-]+$ ]]; then
        echo "error: --version must be a release tag such as v0.1.0" >&2
        exit 2
      fi
      release_version="$1"
      ;;
    --install-dir)
      shift
      if [ "$#" -eq 0 ]; then
        echo "error: --install-dir requires a directory" >&2
        exit 2
      fi
      install_dir="$1"
      install_dir_requested=true
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if [ "$install_binary" = false ] && [ "$install_dir_requested" = true ]; then
  echo "error: --install-dir is only supported with --binary" >&2
  exit 2
fi

# Keep optional model pulls pointed at the same daemon that llm_cli will use.
if [ -n "${LLM_CLI_OLLAMA_HOST:-}" ]; then
  export OLLAMA_HOST="$LLM_CLI_OLLAMA_HOST"
fi

download() {
  local url="$1"
  local destination="$2"
  if command -v curl >/dev/null 2>&1; then
    curl --fail --location --silent --show-error "$url" --output "$destination"
  elif command -v wget >/dev/null 2>&1; then
    wget --quiet --output-document "$destination" "$url"
  else
    echo "error: curl or wget is required to download a release" >&2
    exit 1
  fi
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}/${arch}" in
    Darwin/arm64|Darwin/aarch64) echo "aarch64-apple-darwin" ;;
    Darwin/x86_64) echo "x86_64-apple-darwin" ;;
    Linux/x86_64|Linux/amd64) echo "x86_64-unknown-linux-gnu" ;;
    *)
      echo "error: no published binary target for ${os}/${arch}; install from source with Cargo instead" >&2
      exit 1
      ;;
  esac
}

install_release_binary() {
  local target archive base_url checksum_file expected actual binary
  target="$(detect_target)"
  archive="llm_cli-${target}.tar.gz"
  if [ "$release_version" = "latest" ]; then
    base_url="https://github.com/YingxuanHu/Rust-CLI/releases/latest/download"
  else
    base_url="https://github.com/YingxuanHu/Rust-CLI/releases/download/${release_version}"
  fi

  release_temporary_dir="$(mktemp -d)"
  trap 'rm -rf -- "$release_temporary_dir"' EXIT
  download "${base_url}/${archive}" "${release_temporary_dir}/${archive}"
  checksum_file="${release_temporary_dir}/SHA256SUMS"
  download "${base_url}/SHA256SUMS" "$checksum_file"
  expected="$(awk -v asset="$archive" '$2 == asset { print $1; exit }' "$checksum_file")"
  if [ -z "$expected" ]; then
    echo "error: ${archive} is missing from the published SHA256SUMS file" >&2
    exit 1
  fi
  if command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "${release_temporary_dir}/${archive}" | awk '{print $1}')"
  elif command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "${release_temporary_dir}/${archive}" | awk '{print $1}')"
  else
    echo "error: shasum or sha256sum is required to verify the release archive" >&2
    exit 1
  fi
  if [ "$actual" != "$expected" ]; then
    echo "error: SHA-256 mismatch for ${archive}; refusing to install it" >&2
    exit 1
  fi

  tar -xzf "${release_temporary_dir}/${archive}" -C "$release_temporary_dir"
  binary="${release_temporary_dir}/llm_cli-${target}/llm_cli"
  if [ ! -f "$binary" ]; then
    echo "error: release archive did not contain the expected llm_cli binary" >&2
    exit 1
  fi
  mkdir -p "$install_dir"
  install -m 755 "$binary" "${install_dir}/llm_cli"
  echo "Installed verified llm_cli release to ${install_dir}/llm_cli"
  if [[ ":${PATH}:" != *":${install_dir}:"* ]]; then
    echo "Add ${install_dir} to PATH to run llm_cli from any directory."
  fi
}

if [ "$install_binary" = true ]; then
  install_release_binary
else
  if ! command -v cargo >/dev/null 2>&1; then
    echo "error: Rust/Cargo is required. Install it with rustup: https://rustup.rs" >&2
    echo "hint: use --binary after a release is published to install without Cargo" >&2
    exit 1
  fi

  project_dir="$(cd -- "$(dirname -- "$0")/.." && pwd)"
  echo "Installing llm_cli from ${project_dir}"
  cargo install --path "$project_dir" --locked
fi

if ! command -v ollama >/dev/null 2>&1; then
  cat <<'EOF'

llm_cli is installed, but Ollama is not available yet.
Install Ollama from https://ollama.com, then run:
  llm_cli
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
  cd /path/to/project
  llm_cli

On first run, llm_cli checks its local setup and asks before downloading a
missing chat model.

Optional:
  llm_cli init --full    # Also download routing models
  llm_cli doctor --full  # Inspect setup problems without changing anything
  llm_cli setup          # Create a commented configuration file

Pass --with-models to this installer to pull all default local models now.
EOF
