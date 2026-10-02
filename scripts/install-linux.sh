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
if [[ -f $autostart ]] && ! grep -Eq '^(Hidden=true|X-GNOME-Autostart-enabled=false)$' "$autostart"; then was_enabled=true; fi
state_file="${XDG_STATE_HOME:-$HOME/.local/state}/actualizaciones/state.json"
old_pid=$(pgrep -u "$(id -u)" -f '^/usr/bin/actualizaciones( --open| --autostart)?$' | head -n 1 || true)
is_updating() {
  python3 - "$state_file" <<'PY'
import json, sys
try:
    with open(sys.argv[1]) as file:
        state = json.load(file)
    sys.exit(0 if any(app.get('state') == 'updating' for app in state.get('apps', [])) else 1)
except (OSError, ValueError, TypeError):
    sys.exit(1)
PY
}
if [[ -n $old_pid && -f $state_file ]]; then
  for ((i=0;i<4800;i++)); do
    if is_updating; then sleep .25; else break; fi
  done
  if is_updating; then printf 'Hay una instalación en curso; vuelve a ejecutar el instalador cuando termine.\n' >&2; exit 1; fi
fi
agent_pid=$(pgrep -f '^/usr/bin/actualizaciones-agent$' | head -n 1 || true)
if [[ -n $agent_pid ]]; then
  busctl --user call es.vlad.Actualizaciones.Agent /es/vlad/Actualizaciones/Agent es.vlad.Actualizaciones.Agent1 Shutdown || true
  for ((i=0;i<4800;i++)); do
    if kill -0 "$agent_pid" 2>/dev/null; then sleep .25; else break; fi
  done
  if kill -0 "$agent_pid" 2>/dev/null; then printf 'El agente sigue terminando una instalación; inténtalo de nuevo más tarde.\n' >&2; exit 1; fi
fi
# Stop only the old resident UI, never unrelated WebKit processes.
if [[ -n $old_pid ]]; then
  kill -TERM "$old_pid"
  for ((i=0;i<40;i++)); do kill -0 "$old_pid" 2>/dev/null || break; sleep .25; done
fi
if /usr/bin/pkexec /usr/bin/apt-get install --reinstall -y "$package"; then
  :
else
  status=$?
  if ((status != 127)); then exit "$status"; fi
  # Codex sessions may forbid setuid while Polkit still authorizes systemd.
  systemd-run --system --collect --wait --unit="actualizaciones-install-$$" /usr/bin/apt-get install --reinstall -y "$package"
fi
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
busctl --user call es.vlad.Actualizaciones.Agent /es/vlad/Actualizaciones/Agent es.vlad.Actualizaciones.Agent1 GetState >/dev/null
