#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"
if [[ ! -r /etc/os-release ]] || ! . /etc/os-release || [[ ${ID:-} != ubuntu ]] || [[ ${VERSION_ID:-} != 26.04 ]] || [[ $(uname -m) != x86_64 ]]; then
  printf 'Este build está preparado para Ubuntu 26.04 amd64.\n' >&2; exit 1
fi
for program in node npm cargo rustc pkg-config dpkg-deb; do
  command -v "$program" >/dev/null || { printf 'Falta %s.\n' "$program" >&2; exit 1; }
done
pkg-config --exists webkit2gtk-4.1 gtk+-3.0 || { printf 'Faltan bibliotecas de desarrollo de WebKitGTK.\n' >&2; exit 1; }
npm ci
cargo build --release -p actualizaciones-agent
npm run tauri -- build --bundles deb
package_name=$(node -p 'require("./src-tauri/tauri.conf.json").productName')
package_version=$(node -p 'require("./src-tauri/tauri.conf.json").version')
deb="target/release/bundle/deb/${package_name}_${package_version}_amd64.deb"
[[ -f $deb ]] || { printf 'No se generó el paquete .deb.\n' >&2; exit 1; }
staging=$(mktemp -d)
trap 'rm -rf -- "$staging"' EXIT
dpkg-deb -R "$deb" "$staging"
install -m 0755 target/release/actualizaciones-agent "$staging/usr/bin/actualizaciones-agent"
install -d "$staging/usr/share/dbus-1/services"
install -m 0644 agent/linux/es.vlad.Actualizaciones.Agent.service "$staging/usr/share/dbus-1/services/"
mkdir -p artifacts
dpkg-deb --build --root-owner-group "$staging" "artifacts/$(basename -- "$deb")"
printf 'Paquete listo: %s/artifacts/%s\n' "$project_root" "$(basename -- "$deb")"
