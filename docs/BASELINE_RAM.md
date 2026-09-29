# Medición antes de la migración

29-09-2026, Ubuntu 26.04, sesión GNOME con indicador habilitado. La versión instalada 0.3.0 estaba en reposo, con la ventana cerrada. Una muestra de `/proc/*/smaps_rollup` para su árbol:

| Proceso | RSS KiB | PSS KiB | Privada KiB |
|---|---:|---:|---:|
| actualizaciones | 172556 | 62002 | 40708 |
| WebKitNetworkProcess | 52212 | 12791 | 7656 |
| **Total** | **224768** | **74793** | **48364** |

La app no tenía proceso WebKitWebProcess en ese instante. No es una comparación pareada ni fija un ahorro esperado. El watcher de GNOME `org.kde.StatusNotifierWatcher` estaba activo y el prototipo `ksni` se registró como `org.kde.StatusNotifierItem-202267-1`. El proceso del prototipo no cargó bibliotecas GTK, WebKit ni Tauri según `/proc/<pid>/maps`.
