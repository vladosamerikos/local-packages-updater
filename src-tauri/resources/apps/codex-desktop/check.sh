#!/usr/bin/env bash
set -euo pipefail

readonly repository='https://persistent.oaistatic.com/codex-app-prod/linux/deb'
readonly inrelease_url="$repository/dists/stable/InRelease"
readonly keyring='/usr/share/keyrings/chatgpt-archive-keyring.gpg'
readonly package_name='chatgpt'

for program in curl gpgv sha256sum awk dpkg-query dpkg gzip xz jq; do
  if ! command -v "$program" >/dev/null 2>&1; then
    printf 'Falta el comando necesario: %s\n' "$program" >&2
    exit 1
  fi
done
if [[ ! -r $keyring ]]; then
  printf 'No se encontró la clave de firma del repositorio Codex.\n' >&2
  exit 1
fi

current=$(/usr/bin/dpkg-query -W -f='${Version}' "$package_name" 2>/dev/null) || {
  printf 'Codex Desktop no está instalado como paquete APT chatgpt.\n' >&2
  exit 1
}
if [[ ! $current =~ ^[A-Za-z0-9.+:~_-]+$ ]]; then
  printf 'La versión instalada de Codex Desktop no es válida.\n' >&2
  exit 1
fi

cache_root=${XDG_CACHE_HOME:-"${HOME:?HOME is not set}/.cache"}/actualizaciones/codex-desktop
mkdir -p -- "$cache_root"
chmod 700 -- "$cache_root"
cached_inrelease="$cache_root/InRelease"
cached_etag="$cache_root/InRelease.etag"
tmp_dir=$(mktemp -d -t actualizaciones-codex-XXXXXXXX)
trap 'rm -rf -- "$tmp_dir"' EXIT

curl_args=(--silent --show-error --location --max-time 25)
if [[ -s $cached_inrelease && -s $cached_etag ]]; then
  etag=$(<"$cached_etag")
  if [[ $etag != *$'\r'* && $etag != *$'\n'* ]]; then
    curl_args+=(--header "If-None-Match: $etag")
  fi
fi
http_code=$(/usr/bin/curl "${curl_args[@]}" --dump-header "$tmp_dir/headers" \
  --output "$tmp_dir/InRelease" --write-out '%{http_code}' "$inrelease_url") || {
  printf 'No se pudieron consultar las versiones de Codex Desktop.\n' >&2
  exit 1
}
if [[ $http_code == 304 && -s $cached_inrelease ]]; then
  cp -- "$cached_inrelease" "$tmp_dir/InRelease"
elif [[ $http_code == 200 ]]; then
  :
else
  printf 'El repositorio Codex respondió con HTTP %s.\n' "$http_code" >&2
  exit 1
fi

/usr/bin/gpgv --keyring "$keyring" --output "$tmp_dir/Release" "$tmp_dir/InRelease" \
  >/dev/null 2>&1 || {
  printf 'La firma del índice del repositorio Codex no es válida.\n' >&2
  exit 1
}
if [[ $http_code == 200 ]]; then
  install -m 600 -- "$tmp_dir/InRelease" "$cached_inrelease.new"
  mv -f -- "$cached_inrelease.new" "$cached_inrelease"
  /usr/bin/awk 'tolower($1) == "etag:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); etag=$0 } END { if (etag) print etag }' \
    "$tmp_dir/headers" >"$cached_etag.new"
  chmod 600 -- "$cached_etag.new"
  mv -f -- "$cached_etag.new" "$cached_etag"
fi

index_entry=$(/usr/bin/awk '
  $0 == "SHA256:" { in_sha256=1; next }
  in_sha256 && /^[^[:space:]]/ { exit }
  in_sha256 && $3 == "main/binary-amd64/Packages.xz" { xz=$1 " " $2 " " $3 }
  in_sha256 && $3 == "main/binary-amd64/Packages.gz" { gz=$1 " " $2 " " $3 }
  in_sha256 && $3 == "main/binary-amd64/Packages" { plain=$1 " " $2 " " $3 }
  END { if (xz) print xz; else if (gz) print gz; else if (plain) print plain }
' "$tmp_dir/Release")
if [[ ! $index_entry =~ ^([[:xdigit:]]{64})[[:space:]]+([0-9]+)[[:space:]]+(main/binary-amd64/Packages(\.(xz|gz))?)$ ]]; then
  printf 'El manifiesto firmado no contiene el índice amd64 esperado.\n' >&2
  exit 1
fi
expected_hash=${BASH_REMATCH[1],,}
index_path=${BASH_REMATCH[3]}
case "$index_path" in
  *.xz) index_suffix=xz ;;
  *.gz) index_suffix=gz ;;
  *) index_suffix=plain ;;
esac
package_cache="$cache_root/Packages-$expected_hash.$index_suffix"
if [[ ! -s $package_cache ]]; then
  /usr/bin/curl --fail --silent --show-error --location --max-time 25 \
    "$repository/dists/stable/$index_path" --output "$tmp_dir/Packages.$index_suffix" || {
    printf 'No se pudo descargar el índice de paquetes de Codex.\n' >&2
    exit 1
  }
  actual_hash=$(/usr/bin/sha256sum "$tmp_dir/Packages.$index_suffix")
  actual_hash=${actual_hash%% *}
  if [[ ${actual_hash,,} != "$expected_hash" ]]; then
    printf 'El hash del índice de paquetes de Codex no coincide.\n' >&2
    exit 1
  fi
  install -m 600 -- "$tmp_dir/Packages.$index_suffix" "$package_cache.new"
  mv -f -- "$package_cache.new" "$package_cache"
fi
actual_hash=$(/usr/bin/sha256sum "$package_cache")
actual_hash=${actual_hash%% *}
if [[ ${actual_hash,,} != "$expected_hash" ]]; then
  rm -f -- "$package_cache"
  printf 'El hash de la caché del índice de Codex no coincide.\n' >&2
  exit 1
fi

case "$index_suffix" in
  xz) /usr/bin/xz -dc -- "$package_cache" >"$tmp_dir/Packages" ;;
  gz) /usr/bin/gzip -dc -- "$package_cache" >"$tmp_dir/Packages" ;;
  plain) cp -- "$package_cache" "$tmp_dir/Packages" ;;
esac

latest=$(/usr/bin/awk '
  BEGIN { RS=""; FS="\n" }
  {
    name=""; arch=""; version=""
    for (i=1; i<=NF; i++) {
      if ($i ~ /^Package: /) { name=substr($i,10) }
      else if ($i ~ /^Architecture: /) { arch=substr($i,15) }
      else if ($i ~ /^Version: /) { version=substr($i,10) }
    }
    if (name == "chatgpt" && arch == "amd64") { print version; exit }
  }
  ' "$tmp_dir/Packages")
if [[ ! $latest =~ ^[A-Za-z0-9.+:~_-]+$ ]]; then
  printf 'No se encontró una versión amd64 válida de Codex Desktop.\n' >&2
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
