# Actualizaciones

Pequeña aplicación para Ubuntu 26.04 amd64 que mantiene un indicador de
bandeja y permite comprobar e instalar manualmente actualizaciones de Codex
Desktop y Spotifast. La comprobación inicial se retrasa 90 segundos; después
se repite cada 12 horas. No instala actualizaciones por sí sola.

La ventana tiene un tamaño fijo de 540 × 278 px y utiliza toda su superficie.
«Verificar todas» consulta ambas apps en paralelo. Cada fila permite «Verificar»
únicamente esa app y «Actualizar» cuando existe una versión nueva confirmada.
Las operaciones individuales pueden solaparse entre apps; cada app evita sus
propias llamadas duplicadas. La consulta global espera las operaciones ya
iniciadas y bloquea nuevas acciones individuales hasta terminar. La animación
aparece dentro del botón, que conserva su texto y aspecto mientras trabaja.
El header mide 54 px, la versión y el estado comparten línea, y no hay footer.
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

El nuevo icono azul se generó con image_gen. El maestro transparente, el prompt
y los tamaños exportados están en src-tauri/icons/. Para exportar de nuevo
los tamaños y los dos estados del indicador, ejecuta
python3 scripts/generate-icons.py (requiere Pillow).

Para revisar los estados visuales sin ejecutar instalaciones, inicia `npm run dev`
y abre `http://127.0.0.1:1420/scripts/preview-ui.html`. Esta página usa el frontend
real con el IPC simulado de Tauri y permite revisar errores, comprobaciones y
actualizaciones. No forma parte del paquete de producción.

Las consultas se ejecutan en Rust asíncrono con un cliente HTTP compartido,
tiempos límite, caché ETag y descompresión nativa. El coordinador mantiene una
reserva por app y una señal de finalización para reutilizar las consultas
activas. Los eventos incluyen una revisión creciente; la interfaz ignora
respuestas atrasadas y utiliza directamente el resultado del comando.
Las escrituras del estado se serializan para evitar colisiones concurrentes.

## Integraciones

- Codex Desktop lee la versión de `chatgpt` y verifica `InRelease` con el
  keyring instalado y el hash del índice amd64 antes de extraer el candidato.
  Solo la acción manual usa `pkexec apt-get`; no actualiza Codex CLI.
- Spotifast consulta el release estable de GitHub con ETag. Para instalar,
  descarga el bundle y `checksums.txt`, verifica SHA-256 y llama a Flatpak.
- Los scripts empaquetados tienen rutas fijas y son propiedad de root tras la
  instalación. Los únicos scripts restantes son los instaladores manuales. La interfaz envía
  únicamente el identificador de una app permitida; Rust valida la versión
  candidata y la entrega al instalador de Spotifast.

El estado se guarda en `$XDG_STATE_HOME/actualizaciones/state.json` o en
`~/.local/state/actualizaciones/state.json`. Las respuestas de GitHub y los
metadatos APT firmados se guardan en caché bajo `$XDG_CACHE_HOME`.
