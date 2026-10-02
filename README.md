# Actualizaciones

Utilidad para Ubuntu 26.04 amd64 que comprueba si hay nuevas versiones de **Codex Desktop** y **Spotifast**. Las instalaciones se inician siempre a mano.

Un agente Rust ligero mantiene el icono de la bandeja y hace una comprobación 90 segundos después de arrancar y cada 12 horas. La ventana Tauri se abre cuando la necesitas y termina al cerrarla; las operaciones en curso siguen en el agente.

## Uso

- **Abrir** desde el lanzador o la bandeja para ver versiones y avisos.
- **Verificar** una app o **Verificar todas** para consultar las dos en paralelo.
- **Actualizar** para instalar una versión nueva confirmada. Codex usa Polkit; Spotifast usa Flatpak.
- La bandeja también permite comprobar, activar o desactivar el inicio automático y salir.

El agente conserva el estado en `~/.local/state/actualizaciones/state.json` y la caché de red en `~/.cache/actualizaciones/` (o en las rutas XDG equivalentes).

## Compilar e instalar

Necesitas Ubuntu 26.04 amd64, Rust, Node.js y npm. Para instalar las bibliotecas de desarrollo:

```bash
./scripts/install-build-dependencies.sh
```

Después:

```bash
./scripts/build-linux.sh
./scripts/install-linux.sh
```

El build deja un único `.deb` en `artifacts/`, con el agente, la ventana, los iconos y el servicio D-Bus. El instalador solicita autenticación Polkit, conserva la preferencia de inicio automático y arranca el agente.

## Código

- `core/`: comprobaciones, estado, caché e instalaciones.
- `agent/`: bandeja SNI, temporizador y servicio D-Bus. No carga GTK ni WebKit.
- `src-tauri/` y `src/`: ventana Tauri y frontend.

El plan de la migración y las mediciones de memoria están en `docs/`. Para previsualizar la interfaz sin instalar nada, ejecuta `npm run dev` y abre `/scripts/preview-ui.html` en el servidor local.
