# Plan de migración: Tauri bajo demanda y agente Rust ligero

Fecha: 29 de septiembre de 2026.
Estado: propuesta de implementación. Este documento no modifica la aplicación instalada.
Ámbito: Ubuntu 26.04 amd64. La interfaz HTML/CSS/TypeScript continúa en Tauri 2.

## 1. Objetivo y fundamento

Reducir el consumo sostenido de RAM durante las horas en que solo se necesitan
la bandeja y las comprobaciones periódicas. La ventana debe cargar Tauri y
WebKit únicamente cuando se abre y liberar sus procesos al cerrarse.

Separar tareas en hilos no basta: los hilos comparten el mismo proceso y sus
bibliotecas cargadas. Se necesitan ejecutables y procesos independientes, y el
agente residente debe compilarse sin Tauri, Wry, WebKit ni GTK.

La aplicación actual ya tiene windows: [] y crea la ventana desde open_window().
Al cerrarla destruye la ventana, pero su bucle evita salir del proceso Tauri.
Por tanto, crear la ventana bajo demanda ya está parcialmente resuelto; lo que
falta es separar y terminar el proceso gráfico completo.

En la medición anterior, el árbol de la app ocupaba aproximadamente 219 MiB RSS,
74 MiB PSS y 47 MiB de memoria privada. Es una muestra, no una medición pareada
entre ventana abierta y cerrada. No permite afirmar que se recuperarán exactamente
150 MiB. RSS incluye páginas compartidas; PSS será la métrica principal para
comparar el coste atribuible a la app, junto con memoria privada, RSS y procesos.

Con la ventana abierta coexistirán agente y UI, y el pico podría ser parecido al
actual o ligeramente mayor. La mejora principal se espera en reposo y después
de cerrar la ventana. También existe un coste de arranque al volver a abrirla.

## 2. Arquitectura propuesta

    Inicio de sesión / activación D-Bus
                  |
                  v
    actualizaciones-agent (Rust residente)
      - bandeja StatusNotifierItem mediante D-Bus
      - temporizador inicial de 90 s y comprobaciones cada 12 h
      - cliente HTTP, consultas firmadas y caché
      - coordinador de operaciones por app
      - instalaciones manuales, estado y persistencia
                  |
         D-Bus de la sesión del usuario
                  |
                  v
    actualizaciones (Tauri, solo mientras la ventana está abierta)
      - interfaz y diseños actuales
      - adaptador de comandos y eventos hacia el agente
      - una única ventana
      - salida completa al cerrar

### División del código

Convertir el proyecto Rust en un workspace con límites de dependencias claros:

- actualizaciones-core: modelos, consultas, operaciones, persistencia e instalación.
  Independiente de AppHandle, eventos Tauri, ventanas y bandeja.
- actualizaciones-agent: servicio de sesión, temporizador, bandeja e IPC.
  Depende del core; no enlaza el motor gráfico.
- src-tauri: ventana y adaptador IPC. No ejecuta consultas ni mantiene temporizadores.

Se conservan los iconos, el diseño, el tamaño fijo, los instaladores permitidos,
los checks paralelos, la reutilización de consultas activas y los bloqueos por app.
Solo el agente escribe state.json y administra las operaciones; la UI recibe snapshots.

## 3. Bandeja residente sin motor gráfico

Usar ksni, implementación Rust de StatusNotifierItem, con Tokio. Permite registrar
un icono y un menú mediante D-Bus sin construir una app Tauri residente.

El primer hito será confirmar compatibilidad con la extensión de indicadores de
GNOME instalada en este ordenador: icono, menú, estados normal/pendiente, Abrir,
Comprobar, inicio automático y Salir. El agente debe soportar que el watcher de la
bandeja aparezca tarde o se reinicie.

Esta compatibilidad es un requisito antes de sustituir la bandeja actual. Si el
prototipo no funciona en esta sesión, resolver la integración SNI antes del cambio.
No introducir GTK4 ni otro motor de interfaz para suplirla.

Abrir ejecuta la ruta instalada y fija del binario Tauri. Su mecanismo de instancia
única enfoca la ventana existente cuando ya está abierta. No se crea otra bandeja.
Comprobar funciona directamente en el agente, sin lanzar Tauri.

## 4. Comunicación entre agente y ventana

Usar zbus y el bus D-Bus de sesión. Nombre propuesto:

    es.vlad.Actualizaciones.Agent
    /es/vlad/Actualizaciones/Agent
    interfaz es.vlad.Actualizaciones.Agent1

La adquisición exclusiva del nombre evita agentes duplicados. Empaquetar un
archivo de activación D-Bus para iniciar el agente si la UI se abre antes que él.
El agente se ejecuta como el usuario, nunca como root.

Contrato mínimo versionado:

- GetState: snapshot inicial, versión del protocolo e identificador de sesión del agente.
- StartCheckApp: identificador de app permitido; devuelve aceptación y operationId.
- StartCheckAll: devuelve aceptación y operationId; une operaciones ya activas.
- StartUpdateApp: acción manual; devuelve aceptación y operationId.
- StateChanged: snapshot con instanceId y revision creciente.
- OperationFinished: resultado de operationId, incluido el error de la operación.
- Shutdown: cierre coordinado desde la bandeja.

Las llamadas Start* responden inmediatamente: una instalación puede durar 20 minutos
y no debe depender del tiempo límite de una llamada D-Bus ni de que la ventana siga
abierta. La operación pertenece al agente. Un requestId evita duplicaciones si se
pierde la respuesta de aceptación y la UI reintenta.

Mantener la API invoke del frontend mediante un adaptador Rust en Tauri y reenviar
las señales a state-changed. Suscribir señales antes de pedir GetState; aplicar solo
snapshots con revisión más reciente para la misma instanceId. Si reinicia el agente,
aceptar la nueva instanceId, limpiar pendientes antiguos y volver a sincronizar.
No hacer polling ni pedir GetState después de cada acción completada.

El contrato acepta únicamente apps y acciones previstas, sin rutas o comandos
arbitrarios. Validar versión de protocolo, entradas y límites de payload. La conexión
es local en el bus de la sesión; no requiere un servidor HTTP o puerto TCP.

## 5. Ciclo de vida y comportamiento

| Acción | Agente | Tauri / WebKit |
|---|---|---|
| Inicio de sesión | Arranca y publica la bandeja | No arranca |
| Comprobación periódica o desde bandeja | Ejecuta consultas | No arranca |
| Abrir desde bandeja o lanzador | Arranca si hace falta | Arranca o enfoca la ventana existente |
| Cerrar la ventana | Continúa | Termina por completo |
| Cerrar durante una instalación | Conserva la operación y su progreso | Termina |
| Volver a abrir | Entrega el estado actual | Reconstruye la vista desde el snapshot |
| Salir desde bandeja | Cierre coordinado | Se cierra si está abierta |

En la UI eliminar la prevención incondicional de ExitRequested y permitir la salida
normal al cerrar la última ventana. Comprobar que sus procesos WebKit desaparecen
sin matar indiscriminadamente otros procesos WebKit del sistema.

En el agente, Salir debe coordinar las operaciones activas. Una instalación manual
ya iniciada debe terminar antes de salir; una consulta puede cancelarse. Un cierre de
UI nunca cancela ni duplica una instalación. SIGTERM y recuperación tras interrupción
requieren un manejo explícito en el agente.

El inicio automático debe ejecutar actualizaciones-agent --autostart. Migrar solo
la entrada de esta app y conservar la elección actual del usuario. El lanzador de
escritorio sigue abriendo actualizaciones --open.

## 6. Optimización adicional, después de separar procesos

Orden por impacto esperado:

1. Eliminar WebKit y Tauri del agente y garantizar salida completa de la UI.
2. Mantener un único runtime Tokio pequeño y un único cliente HTTP por agente;
   dimensionar hilos y conexiones para dos apps, conservando la concurrencia actual.
3. Liberar índices descomprimidos, respuestas y resultados temporales al terminar;
   mantener caché de red en disco y límites de descarga existentes. Ajustar conexiones
   inactivas solo si la medición muestra un beneficio.
4. UI sin polling, fuentes remotas, frameworks extra ni precarga de ventanas.
   Actualizar únicamente las filas y propiedades cambiadas en vez de recrear toda
   la lista. Reutilizar los SVG de acciones y decodificar iconos a resolución útil.
5. Retirar plugins de bandeja y autostart del binario Tauri cuando se hayan trasladado
   al agente. Conservar instancia única y permisos necesarios.
6. Evaluar perfiles release, LTO y eliminación de símbolos por separado: reducen
   principalmente el tamaño del ejecutable y no sustituyen la separación de procesos.

No aplicar flags de WebKit, desactivar aceleración o cambiar asignadores de memoria
sin un resultado medido que justifique el cambio. El número de tarjetas o el tamaño
del header no explica por sí solo el consumo base del motor web.

## 7. Fases y commits previstos

### Fase A — Línea base y prototipo de bandeja

Documentar memoria y procesos en reposo, ventana abierta, comprobando y tras cerrar.
Crear un prototipo SNI separado y comprobar su integración en GNOME. Confirmar que
su proceso no carga GTK/WebKit/Tauri. La app estable sigue instalada durante esta fase.

Commit previsto: prototipo de bandeja D-Bus y mediciones de referencia.

### Fase B — Extraer el core

Mover AppStatus, StoredState, UiSnapshot, Checkers, Coordinator, instalación y guardado
a la biblioteca. Sustituir AppHandle por interfaces de publicación y rutas explícitas.
Mantener de forma transitoria la app actual usando la biblioteca para aislar cambios.

Commit previsto: núcleo Rust desacoplado de Tauri.

### Fase C — Construir el agente

Integrar core, D-Bus, bandeja, temporizador, instancia exclusiva y cierre coordinado.
Preservar caché, claves APT, timeouts, grupos de procesos, Polkit y confirmación de versión.

Commit previsto: agente residente y protocolo de sesión.

### Fase D — Convertir Tauri en cliente efímero

Añadir adaptador D-Bus y suscripción de eventos. Trasladar autostart y retirar tray,
temporizador e instalaciones del proceso gráfico. Permitir salida completa al cerrar.
Mantener diseño, animaciones de botones y paralelismo de checks individuales.

Commit previsto: ventana Tauri bajo demanda con cierre completo.

### Fase E — Build, empaquetado y migración

Compilar agente y Tauri en un solo script y generar un único .deb con los dos binarios,
iconos, recursos y activación D-Bus. El agente necesita rutas de recursos instaladas
explícitas; no dependerá del resolvedor de recursos Tauri.

Adaptar el instalador para detener de forma coordinada la versión anterior, migrar su
entrada autostart sin cambiar la preferencia, instalar y arrancar únicamente el agente.
Evitar dos bandejas o dos temporizadores durante el cambio. Mantener disponible el
.deb 0.3.0 para volver atrás y conservar un estado compatible o respaldarlo si cambia.

Commit previsto: paquete Linux y migración de autostart.

### Fase F — Comparación y ajustes

Aplicar solo optimizaciones adicionales con resultados favorables. Publicar una tabla
antes/después con las condiciones y el total del árbol de procesos. Compilar e instalar
la versión final después de completar las fases anteriores.

Commit previsto: ajustes de consumo y resultados de migración.

## 8. Criterios de aceptación y medición futura

Este plan propone las siguientes verificaciones para la implementación; no se han
ejecutado en esta fase de documentación.

- Sin ventana abierta: solo el agente residente; sin procesos WebKit propios ni
  bibliotecas WebKit/Tauri/GTK en el agente. Las dependencias de la sesión de escritorio
  ya existente no se atribuyen enteras a la app.
- Comparar RSS, PSS y memoria privada sumando todos los procesos de la app; muestrear
  varias veces con tiempos de reposo definidos y las mismas condiciones de sesión.
- Ventana abierta: incluir agente, Tauri y auxiliares WebKit. Documentar tiempo de
  primera apertura y reapertura además de RAM y CPU en reposo.
- Tras cerrar y estabilizar: volver al consumo del agente, sin crecimiento sostenido
  al repetir abrir/cerrar 20 veces. Separar calentamiento de caché de posible fuga.
- Consultas individuales simultáneas, consulta global durante una individual, clicks
  duplicados y errores mantienen el comportamiento de la versión actual.
- Cerrar durante una comprobación o instalación no cancela ni repite la operación;
  al volver a abrir se muestra su estado correcto.
- Abrir varias veces deja una única UI y un único agente. Reiniciar agente o bandeja
  restablece la conexión y la revisión sin bloquear botones permanentemente.
- Inicio de sesión y checks periódicos no lanzan la ventana. Se conserva el ajuste
  de autostart y las versiones/error actuales del estado persistido.

El éxito exige una reducción demostrable del PSS y memoria privada en reposo junto
con el aislamiento completo del motor web. El ahorro exacto en MiB se fija a partir
de la comparación, no del supuesto de 150 MiB. Con la ventana abierta sigue existiendo
el coste de Tauri/WebKit; la separación optimiza especialmente la mayor parte del
tiempo en que la utilidad permanece en la bandeja.

## 9. Referencias técnicas

- [Modelo de procesos de Tauri](https://tauri.app/concept/process-model/).
- [ksni: bandeja StatusNotifierItem en Rust](https://docs.rs/ksni/latest/ksni/).
- [zbus: comunicación D-Bus en Rust](https://docs.rs/zbus/latest/zbus/).
- [Especificación D-Bus y activación de servicios](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-starting-services).
- [Perfiles de compilación Cargo](https://doc.rust-lang.org/cargo/reference/profiles.html).

Estas fuentes respaldan las capacidades de las herramientas. La elección de
arquitectura y la expectativa de ahorro son propuestas de este plan; el resultado
concreto requiere medir la implementación en este ordenador.
