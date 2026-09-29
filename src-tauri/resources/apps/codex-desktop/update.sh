#!/usr/bin/env bash
set -euo pipefail

if ! /usr/bin/dpkg-query -W -f='${Status}' chatgpt 2>/dev/null | /usr/bin/grep -qx 'install ok installed'; then
  printf 'Codex Desktop no está instalado como paquete APT chatgpt.\n' >&2
  exit 1
fi
if [[ ! -x /usr/bin/pkexec || ! -x /usr/bin/apt-get ]]; then
  printf 'No se encontró pkexec o apt-get para actualizar Codex Desktop.\n' >&2
  exit 1
fi

/usr/bin/pkexec /usr/bin/apt-get update
/usr/bin/pkexec /usr/bin/apt-get install --only-upgrade -y chatgpt
printf 'Codex Desktop se actualizó. Reinicia la aplicación si estaba abierta.\n'
