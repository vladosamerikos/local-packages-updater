#!/usr/bin/env bash
set -euo pipefail

if [[ ! -r /etc/os-release ]] || ! . /etc/os-release || [[ ${ID:-} != ubuntu ]]; then
  printf 'Este instalador de dependencias está preparado para Ubuntu.\n' >&2
  exit 1
fi

exec sudo /usr/bin/apt-get install -y \
  build-essential \
  cargo \
  curl \
  file \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  liblzma-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  nodejs \
  npm \
  pkg-config \
  python3-pil \
  rustc
