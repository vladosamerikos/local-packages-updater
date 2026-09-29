# Actualizaciones

Pequeña aplicación para Ubuntu 26.04 amd64 que mantiene un indicador de
bandeja y permite comprobar e instalar manualmente actualizaciones de Codex
Desktop y Spotifast. La comprobación inicial se retrasa 90 segundos; después
se repite cada 12 horas. No instala actualizaciones por sí sola.

La ventana tiene un tamaño fijo de 540 × 390 px y utiliza toda su superficie.
«Verificar todas» consulta ambas apps. Cada fila permite «Verificar» únicamente
esa app y «Actualizar» cuando existe una versión nueva confirmada. El estado
aparece una sola vez por fila, conservando la versión instalada al verificar.
Las filas usan los iconos incluidos por las instalaciones originales:
`src/assets/codex-desktop.png` y `src/assets/spotifast.svg`.

## Compilar en Linux

```sh
./scripts/install-build-dependencies.sh
npm ci
./scripts/build-linux.sh
```

El `.deb` queda en `artifacts/`. Para instalarlo desde GNOME con autenticación
Polkit:

```sh
./scripts/install-linux.sh
```

La compilación está limitada a Ubuntu amd64 con Tauri 2, Node.js, npm y Rust.
Los paquetes de desarrollo se pueden instalar de forma opcional con el primer
script; este no se ejecuta automáticamente al compilar.

Para rehacer el icono de la app y los dos estados del indicador desde el código
fuente, ejecuta `python3 scripts/generate-icons.py` (requiere Pillow).

Para revisar los estados visuales sin ejecutar instalaciones, inicia `npm run dev`
y abre `http://127.0.0.1:1420/scripts/preview-ui.html`. Esta página usa el frontend
real con el IPC simulado de Tauri y permite revisar errores, comprobaciones y
actualizaciones. No forma parte del paquete de producción.

Las regresiones del estado y de los errores se comprueban con
`cargo test --release --manifest-path src-tauri/Cargo.toml`. Las comprobaciones
reales se pueden ejecutar directamente con Bash desde `src-tauri/resources/apps/`.

## Integraciones

- Codex Desktop lee la versión de `chatgpt` y verifica `InRelease` con el
  keyring instalado y el hash del índice amd64 antes de extraer el candidato.
  Solo la acción manual usa `pkexec apt-get`; no actualiza Codex CLI.
- Spotifast consulta el release estable de GitHub con ETag. Para instalar,
  descarga el bundle y `checksums.txt`, verifica SHA-256 y llama a Flatpak.
- Los scripts empaquetados tienen rutas fijas y son propiedad de root tras la
  instalación. La interfaz nunca proporciona comandos ni argumentos.

El estado se guarda en `$XDG_STATE_HOME/actualizaciones/state.json` o en
`~/.local/state/actualizaciones/state.json`. Las respuestas de GitHub y los
metadatos APT firmados se guardan en caché bajo `$XDG_CACHE_HOME`.
