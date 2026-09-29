#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"

if [[ ! -r /etc/os-release ]] || ! . /etc/os-release || [[ ${ID:-} != ubuntu ]] || [[ ${VERSION_ID:-} != 26.04 ]] || [[ $(uname -m) != x86_64 ]]; then
  printf 'Este build está preparado para Ubuntu 26.04 amd64.\n' >&2
  exit 1
fi

missing=()
for program in node npm cargo rustc pkg-config; do
  if ! command -v "$program" >/dev/null 2>&1; then missing+=("$program"); fi
done
if ((${#missing[@]})); then
  printf 'Faltan herramientas: %s\n' "${missing[*]}" >&2
  printf 'Ejecuta scripts/install-build-dependencies.sh e instala Rust y Node.js.\n' >&2
  exit 1
fi
if ! pkg-config --exists webkit2gtk-4.1 gtk+-3.0 ayatana-appindicator3-0.1; then
  printf 'Faltan bibliotecas de desarrollo de WebKitGTK 4.1 o Ayatana AppIndicator.\n' >&2
  printf 'Ejecuta scripts/install-build-dependencies.sh.\n' >&2
  exit 1
fi

npm ci
npm run tauri -- build --bundles deb

package_name=$(node -p 'require("./src-tauri/tauri.conf.json").productName')
package_version=$(node -p 'require("./src-tauri/tauri.conf.json").version')
deb="src-tauri/target/release/bundle/deb/${package_name}_${package_version}_amd64.deb"
if [[ ! -f $deb ]]; then
  printf 'Tauri terminó sin generar el paquete .deb esperado.\n' >&2
  exit 1
fi
mkdir -p artifacts
cp -- "$deb" "artifacts/$(basename -- "$deb")"
printf 'Paquete listo: %s/artifacts/%s\n' "$project_root" "$(basename -- "$deb")"
