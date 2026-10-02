# Resultado de la separación agente/UI

Medición del 2 de octubre de 2026 en Ubuntu 26.04 amd64, GNOME/Wayland. Versión instalada: 0.4.0. Se sumaron `Rss`, `Pss` y `Private_Clean` + `Private_Dirty` de `/proc/<pid>/smaps_rollup` para cada proceso de la aplicación y sus descendientes con `scripts/measure-memory.py`. Las cifras nuevas son medianas de tres muestras separadas por dos segundos, tras tres segundos de estabilización; las de 0.3.0 son muestras únicas tomadas el 29 de septiembre en la misma sesión de escritorio. Son orientativas, no una comparación perfectamente pareada.

| Estado | Versión | Procesos de la app | RSS MiB | PSS MiB | Privada MiB |
|---|---|---:|---:|---:|---:|
| Ventana cerrada, reposo | 0.3.0 | 2 (Tauri y WebKitNetworkProcess) | 219,5 | 73,0 | 47,2 |
| Ventana abierta | 0.3.0 | 3 | 413,3 | 179,6 | 130,9 |
| Ventana cerrada, tras consultas y 20 ciclos | 0.4.0 | 1 (agente) | 15,5 | 12,8 | 12,8 |
| Ventana abierta | 0.4.0 | 4 (agente, Tauri y dos WebKit) | 414,1 | 216,6 | 134,7 |
| Tras cerrar y estabilizar | 0.4.0 | 1 (agente) | 15,5 | 12,8 | 12,8 |

Antes de las primeras consultas, el agente 0.4.0 medía 8.724 KiB RSS, 6.147 KiB PSS y 6.124 KiB privados. La caché y el cliente HTTP elevaron el reposo posterior a unos 13.155 KiB PSS, que se mantuvieron estables. Frente a la muestra anterior de reposo, la diferencia orientativa es de unos **60 MiB PSS** y **34 MiB privados**. Con ventana abierta el PSS fue mayor que en la muestra de 0.3.0; el objetivo de esta arquitectura es el reposo sin ventana.

## Verificaciones

- El watcher de GNOME registró el SNI del agente; el menú D-Bus expuso Abrir, Comprobar, inicio automático y Salir. Se activó Comprobar desde el menú sin lanzar la UI.
- El agente no cargó GTK, WebKit, Tauri ni Ayatana según `ldd` y `/proc/<pid>/maps`.
- Dos consultas individuales se ejecutaron simultáneamente. La consulta global se unió a ambas; un `requestId` repetido devolvió el mismo `operationId`; una app desconocida fue rechazada.
- El paquete contiene el servicio de activación D-Bus, ambos binarios, los scripts permitidos, el icono y `es.vlad.actualizaciones.desktop`. GTK registró `es.vlad.actualizaciones` mientras la UI estaba abierta, de modo que GNOME puede asociarla al icono del lanzador.
- Abrir/cerrar la ventana 20 veces terminó sin Tauri ni WebKit residuales. El PSS del agente en los ciclos 1, 5, 10 y 20 fue 13.156, 13.156, 13.155 y 13.156 KiB. La aparición del proceso WebKitWebProcess tardó 574 ms en la primera apertura tras reiniciar el agente y 584 ms en una reapertura con cachés calientes; esto no mide el tiempo hasta pintar toda la interfaz.
- Se conservó la preferencia de inicio automático desactivado. Las consultas firmadas de Codex y la caché funcionaron desde el agente. No había actualizaciones pendientes, así que no se ejecutó una instalación real.

Una primera llamada de activación D-Bus agotó el tiempo de espera durante el desarrollo. Se cambió el orden para exportar el objeto antes de adquirir el nombre. Tras reinstalar el paquete recompilado, la activación completó `GetState` en 32 ms.
