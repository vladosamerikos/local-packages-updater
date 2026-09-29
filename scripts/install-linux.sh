#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if (($# > 1)); then
  printf 'Uso: %s [paquete.deb]\n' "$0" >&2
  exit 2
fi
package=${1:-}
if [[ -z $package ]]; then
  package=$(find "$project_root/artifacts" -maxdepth 1 -type f -name '*.deb' -print 2>/dev/null | sort -V | tail -n 1 || true)
fi
if [[ -z $package || ! -f $package ]]; then
  printf 'No se encontró un .deb. Ejecuta scripts/build-linux.sh primero.\n' >&2
  exit 1
fi
package=$(realpath -- "$package")
exec /usr/bin/pkexec /usr/bin/apt-get install -y "$package"
