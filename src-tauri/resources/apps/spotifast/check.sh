#!/usr/bin/env bash
set -euo pipefail

readonly app_id='rocks.spotifast.Spotifast'
readonly api_url='https://api.github.com/repos/crmne/spotifast/releases?per_page=30'

for program in curl jq flatpak; do
  if ! command -v "$program" >/dev/null 2>&1; then
    printf 'Falta el comando necesario: %s\n' "$program" >&2
    exit 1
  fi
done

arch=$(uname -m)
if [[ $arch != x86_64 ]]; then
  printf 'Arquitectura no compatible: %s\n' "$arch" >&2
  exit 1
fi

current=$(/usr/bin/flatpak list --user --app --columns=application,version |
  /usr/bin/awk -F '\t' -v id="$app_id" '$1 == id { print $2; exit }')
if [[ ! $current =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][A-Za-z0-9.-]+)?$ ]]; then
  printf '%s no está instalado o su versión no es válida.\n' "$app_id" >&2
  exit 1
fi

cache_root=${XDG_CACHE_HOME:-"${HOME:?HOME is not set}/.cache"}/actualizaciones/spotifast
mkdir -p -- "$cache_root"
chmod 700 -- "$cache_root"
cache_json="$cache_root/releases.json"
cache_etag="$cache_root/releases.etag"
tmp_dir=$(mktemp -d -t actualizaciones-spotifast-XXXXXXXX)
trap 'rm -rf -- "$tmp_dir"' EXIT

curl_args=(--silent --show-error --location --max-time 25 --max-filesize 5000000 \
  --header 'Accept: application/vnd.github+json' \
  --header 'User-Agent: actualizaciones-linux')
if [[ -s $cache_json && -s $cache_etag ]]; then
  etag=$(<"$cache_etag")
  if [[ $etag != *$'\r'* && $etag != *$'\n'* ]]; then
    curl_args+=(--header "If-None-Match: $etag")
  fi
fi

http_code=$(/usr/bin/curl "${curl_args[@]}" --dump-header "$tmp_dir/headers" \
  --output "$tmp_dir/releases.json" --write-out '%{http_code}' "$api_url") || {
  printf 'No se pudieron consultar las versiones de Spotifast.\n' >&2
  exit 1
}
if [[ $http_code == 304 && -s $cache_json ]]; then
  cp -- "$cache_json" "$tmp_dir/releases.json"
elif [[ $http_code == 200 ]]; then
  /usr/bin/jq -e 'type == "array"' "$tmp_dir/releases.json" >/dev/null || {
    printf 'GitHub devolvió una lista de versiones no válida.\n' >&2
    exit 1
  }
  install -m 600 -- "$tmp_dir/releases.json" "$cache_json.new"
  mv -f -- "$cache_json.new" "$cache_json"
  /usr/bin/awk 'tolower($1) == "etag:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); etag=$0 } END { if (etag) print etag }' \
    "$tmp_dir/headers" >"$cache_etag.new"
  chmod 600 -- "$cache_etag.new"
  mv -f -- "$cache_etag.new" "$cache_etag"
else
  printf 'GitHub respondió con HTTP %s al consultar Spotifast.\n' "$http_code" >&2
  exit 1
fi

release=$(/usr/bin/jq -cer --arg arch "$arch" '
  first(.[]
    | select(.draft == false and .prerelease == false)
    | select(.tag_name | test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))
    | . as $r
    | select(any($r.assets[]; .name == ("spotifast-" + $r.tag_name + "-" + $arch + ".flatpak")))
    | select(any($r.assets[]; .name == "checksums.txt"))
  )
' "$tmp_dir/releases.json") || {
  printf 'No se encontró una versión estable de Spotifast con el bundle y sus checksums.\n' >&2
  exit 1
}

latest=$(/usr/bin/jq -er '.tag_name | capture("^v(?<version>[0-9]+\\.[0-9]+\\.[0-9]+)$").version' <<<"$release")
if [[ ! $latest =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'GitHub devolvió una versión de Spotifast no válida.\n' >&2
  exit 1
fi

if /usr/bin/dpkg --compare-versions "$latest" gt "$current"; then
  available=true
else
  available=false
fi

/usr/bin/jq -cn --arg installed "$current" --arg latest "$latest" \
  --argjson update_available "$available" \
  '{installed:$installed,latest:$latest,update_available:$update_available}'
