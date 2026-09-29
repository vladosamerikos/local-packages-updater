#!/usr/bin/env bash
set -euo pipefail

for program in curl flatpak sha256sum awk; do
  if ! command -v "$program" >/dev/null 2>&1; then
    printf 'Falta el comando necesario: %s\n' "$program" >&2
    exit 1
  fi
done

latest=${ACTUALIZACIONES_TARGET_VERSION:-}
if [[ ! $latest =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'La versión de Spotifast no es válida.\n' >&2
  exit 1
fi
tag="v$latest"
filename="spotifast-${tag}-x86_64.flatpak"
base_url="https://github.com/crmne/spotifast/releases/download/${tag}"
tmp_dir=$(mktemp -d -t actualizaciones-spotifast-update-XXXXXXXX)
trap 'rm -rf -- "$tmp_dir"' EXIT

/usr/bin/curl --fail --silent --show-error --location --retry 3 --max-time 300 \
  "$base_url/$filename" --output "$tmp_dir/$filename"
/usr/bin/curl --fail --silent --show-error --location --retry 3 --max-time 60 \
  "$base_url/checksums.txt" --output "$tmp_dir/checksums.txt"

expected=$(/usr/bin/awk -v name="$filename" '$2 == name || $2 == "*" name { print $1; exit }' \
  "$tmp_dir/checksums.txt")
if [[ ! $expected =~ ^[[:xdigit:]]{64}$ ]]; then
  printf 'No hay un checksum SHA-256 válido para %s.\n' "$filename" >&2
  exit 1
fi
actual=$(/usr/bin/sha256sum "$tmp_dir/$filename")
actual=${actual%% *}
if [[ ${actual,,} != ${expected,,} ]]; then
  printf 'El checksum de Spotifast no coincide; se canceló la instalación.\n' >&2
  exit 1
fi

/usr/bin/flatpak install --user --or-update --bundle -y "$tmp_dir/$filename"
printf 'Spotifast %s se instaló correctamente.\n' "$latest"
