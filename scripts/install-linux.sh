#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if (($# > 1)); then printf 'Uso: %s [paquete.deb]\n' "$0" >&2; exit 2; fi
package=${1:-}
if [[ -z $package ]]; then
  package=$(find "$project_root/artifacts" -maxdepth 1 -type f -name 'Actualizaciones_*.deb' -print 2>/dev/null | sort -V | tail -n 1 || true)
fi
[[ -f $package ]] || { printf 'No se encontró el .deb.\n' >&2; exit 1; }
package=$(realpath -- "$package")
autostart="$HOME/.config/autostart/Actualizaciones.desktop"
was_enabled=false
[[ -f $autostart ]] && was_enabled=true
state_file="${XDG_STATE_HOME:-$HOME/.local/state}/actualizaciones/state.json"
if [[ -f $state_file ]]; then
  for ((i=0;i<4800;i++)); do
    if ! python3 - "$state_file" <<'PY'
import json,sys
try: print('',end=''); assert not any(app.get('state')=='updating' for app in json.load(open(sys.argv[1])).get('apps',[]))
except Exception: sys.exit(1)
PY
    then sleep .25; else break; fi
  done
fi
# Stop only the old resident UI, never unrelated WebKit processes.
old_pid=$(pgrep -u "$(id -u)" -x actualizaciones | head -n 1 || true)
if [[ -n $old_pid ]]; then
  kill -TERM "$old_pid"
  for ((i=0;i<40;i++)); do kill -0 "$old_pid" 2>/dev/null || break; sleep .25; done
fi
/usr/bin/pkexec /usr/bin/apt-get install --reinstall -y "$package"
if [[ $was_enabled == true ]]; then
  mkdir -p "$(dirname "$autostart")"
  cat > "$autostart" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Actualizaciones
Exec=/usr/bin/actualizaciones-agent --autostart
Terminal=false
X-GNOME-Autostart-enabled=true
DESKTOP
fi
/usr/bin/actualizaciones-agent >/dev/null 2>&1 &
