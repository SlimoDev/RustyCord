# NotDiscord Rust para Windows

Workspace físico: `E:/DESCARGAS/REA/NotDiscordRust`; este proyecto está en su subcarpeta `NotDiscord-Rust` y las herramientas están en `.tools`, junto a ella. Compilación, cachés, datos y entregas permanecen en E:. El traslado conserva el historial y los resultados históricos de pruebas; véase [workspace-migration.json](evidence/workspace-migration.json).

Port de la interfaz y lógica propias del cliente nativo. La referencia C# permanece en `E:/DESCARGAS/REA/NotDiscord/terminal-native`; no se modifica ni se carga para ejecutar esta versión. La documentación consolidada de UI8, con arquitectura, interfaces, funciones, diferencias con Discord y decisiones P0/P1/P2, está en [DOCUMENTACION_COMPLETA_NOTDISCORD_RUST_UI8.md](DOCUMENTACION_COMPLETA_NOTDISCORD_RUST_UI8.md). [DOCUMENTACION_PROYECTO.md](DOCUMENTACION_PROYECTO.md) conserva la consolidación anterior.

## Compilar

Se necesitan Rust estable para Windows x64 MSVC, las herramientas de compilación C++/SDK de Windows y WebView2 Runtime para el inicio de sesión. No se necesita .NET.

```powershell
.\build.ps1 -Test -Release
```

El script usa Cargo instalado en PATH o la instalación local de herramientas del espacio de trabajo. `Cargo.lock` fija las versiones resueltas. No compilar con características de pruebas al distribuir.

Esta copia permite `-Offline` con las dependencias ya disponibles. La compilación usa un trabajo por defecto para limitar la memoria del compilador; se puede elegir otra concurrencia con `-Jobs`.

Para empaquetar y comprobar los binarios nativos y las rutas portables:

```powershell
.\package.ps1
.\verify-package.ps1
```

Estos dos scripts conservan el flujo histórico de empaquetado e informes: `package.ps1` aún recopila resultados UI7 y `verify-package.ps1` escribe un JSON. Para reconstruir UI8 respetando la preferencia de no generar informes, consultar el procedimiento y las verificaciones del capítulo 32 de la [documentación consolidada](DOCUMENTACION_COMPLETA_NOTDISCORD_RUST_UI8.md).

El runtime multimedia independiente ya está preparado en esta copia. Para reconstruirlo desde las dependencias de referencia, `crates/media-addon/Prepare-Host.ps1` acepta `-ReferenceRuntime` y `-ReferenceNativeRoot`, copia sólo el host/motor terceros e instala el bootstrap nuevo; el runtime anterior no se necesita para ejecutar el paquete resultante.

## Ejecutar

La revisión actual es **UI8**, distribuida en `dist/NotDiscord-Rust-20261022-ui8`. El lanzador del proyecto y el acceso directo Rust del escritorio apuntan a esta carpeta, conservando la ruta de datos; UI7 y las entregas anteriores se conservan como respaldo. La aplicación mantiene el nombre **NotDiscord** y los formatos de datos existentes; la referencia C# permanece independiente. El alcance prioritario A17–A18 y sus límites están en [OPTIMIZACIONES_P2.md](docs/OPTIMIZACIONES_P2.md).

```powershell
.\target\release\NotDiscord.exe --runtime-root . --data-dir .\data
```

El directorio de datos puede ser el directorio existente de la referencia, usando una ruta absoluta. DPAPI requiere el mismo usuario de Windows. Para una copia independiente, preservar los archivos cifrados y JSON descritos en la documentación; no incluirlos en un paquete público. La opción `--data-dir` se transmite al proceso de inicio de sesión.

La interfaz es Rust/egui. Sólo el inicio de sesión normal abre WebView2 y su perfil transitorio propio. Voz, cámara y pantalla requieren el motor nativo tercero y el host compatible incluidos/configurados en `runtime.json`; la lógica del helper está en `notdiscord-media-addon`, cargado como `NotDiscord.Media.node`. El bootstrap de Electron se limita a cargar el addon y el motor. No se necesita ni se usa el helper C#/JavaScript anterior para las llamadas.

UI7 permite importar MP3 y OGG/Vorbis una vez a WAV PCM16 mono de 48 kHz, con timbres de hasta un minuto leídos por bloques. Añade Overwatch al catálogo local, menciones editables sin perder IDs, pies fijos de diálogos, selección con scroll, reproducción conjunta de emojis al pasar el puntero y control de recepción/ampliación de vídeo. Sitúa los botones de volver en el encabezado, representa textos de embeds/componentes de bots y conserva su identidad en perfiles. El menú contextual del servidor incluye los modos de notificación y las supresiones solicitadas, además de ocultar canales silenciados. Los detalles y límites están en [CORRECCIONES_UI7.md](docs/CORRECCIONES_UI7.md); las pruebas, mediciones y funciones pendientes de comprobación remota, en [UI7-VALIDACION.md](evidence/UI7-VALIDACION.md).

Los sonidos de mensaje y mención se silencian sólo si ese chat está visible y la ventana tiene el foco en primer plano; en segundo plano o minimizada se mantienen los avisos permitidos por los ajustes. Enter envía el borrador preparado desde el historial aunque el compositor no tenga foco, respetando otros campos, diálogos, IME y Shift+Enter. Con la aplicación ya ejecutándose, pulsar un aviso de Windows restaura y maximiza la ventana y abre su conversación. Windows puede impedir la transferencia del foco; el cliente respeta esa restricción. No se incluye el arranque desde avisos con la aplicación cerrada.

UI6 introdujo las mejoras anteriores de menús, borradores, historial observado, blur, emojis, GIF, detección local de juegos y sonidos. Su evidencia histórica permanece en [CORRECCIONES_UI6.md](docs/CORRECCIONES_UI6.md) y [UI6-VALIDACION.md](evidence/UI6-VALIDACION.md).

UI5 ajustó el blur a 0.00–1.00, bloqueó el fondo de los diálogos, corrigió la geometría y reapertura del visor y separó las previews de los selectores. Renovó la demanda de miembros después de recargar o cambiar permisos; los participantes del historial se identifican como lista parcial cuando no existe una lista remota confirmada. Su evidencia histórica permanece en [UI5-VALIDACION.md](evidence/UI5-VALIDACION.md).

UI4 corrige «Volver al inicio» para aplicar la página reciente y su scroll juntos, mantiene las barras de desplazamiento de grosor fijo y muestra bordes en los campos desde su estado inicial. Añade desenfoque nativo regulable del fondo visible a través de la transparencia; texto, iconos y medios opacos conservan su color. En sistemas Windows que no ofrezcan el efecto se informa del error y permanece la transparencia. El contrato está en [CORRECCIONES_UI4.md](docs/CORRECCIONES_UI4.md).

Los selectores de cámara/pantalla previsualizan una sola selección y la liberan al cerrar o cancelar. Cada participante ofrece dos botones verticales cuando comparte cámara y pantalla; seleccionar uno muestra esa fuente y pulsarlo de nuevo restaura el avatar. Desde una llamada pendiente, mostrar una fuente une a la llamada real conservando silencio y ensordecimiento; ocultarla no abandona la llamada. Las llamadas entrantes abren una ventana nativa superior con Ignorar/Aceptar/Rechazar, salvo No molestar o conversación silenciada. Cerrar esa ventana sólo la ignora: los participantes permanecen en el chat mientras siga activa la llamada. Se retiraron los botones redundantes «Voz», «Pantalla» y «Vista previa local». La emisión saliente continúa condicionada a la preparación cifrada; una preview local no equivale a transmitir.

Los GIF conservan su enlace animado y los errores de carga/decodificación permiten reintentar. Los stickers se resumen mediante su nombre real, los reenvíos muestran «Reenvió un mensaje» y los avisos preparan una imagen local del MD/grupo/servidor para Windows. Se conservan las mejoras UI3: historial compacto, paneles independientes, respuestas escaladas, adjuntos preparados, reenvíos de hasta diez destinos y lectura confirmada por visibilidad.

## Verificar

El cierre ampliado de RAM, multimedia local y emojis de Windows se documenta en [P2-CIERRE-VALIDACION.md](evidence/P2-CIERRE-VALIDACION.md), con muestras crudas y límites de interpretación. La evaluación del acceso autorizado está en [MODELO_DE_ACCESO_AUTORIZADO.md](docs/MODELO_DE_ACCESO_AUTORIZADO.md): A01 sigue pendiente y no se ha migrado el login. Los ensayos prolongados son optativos; no forman parte del arranque del cliente.

```powershell
cargo test --workspace
.\crates\media-addon\Test-Host.ps1
```

Las pruebas automáticas usan fixtures y no envían mensajes, solicitudes de amistad ni inician dispositivos o llamadas reales. Un PNG de prueba de interfaz se obtiene con `--preview ruta-absoluta.png --fixture evidence/ui-fixture.json --data-dir evidence/preview-data`; ese modo no conecta una cuenta ni acredita funciones reales de red.

P2 comparte las listas de mensajes y miembros entre snapshots, copia sus datos al modificarlos y evita reconstruir mensajes completos para respuestas o búsquedas puntuales. Libera buffers de vistas retiradas y del cierre de sesión. Las imágenes estáticas se descargan a temporales acotados y se decodifican sin retener sus cuerpos completos en cada trabajador; los avatares usan resolución física y una caché de disco por cuenta con cuota y caducidad. Se preservan IDs string, JSON, borradores y límites existentes. Pasaron **456 pruebas offline, cero fallidas y dos excluidas por defecto**. Las mediciones reproducibles, regresiones y hashes exactos están en [P2-VALIDACION.md](evidence/P2-VALIDACION.md); distinguen asignaciones de memoria del proceso y no usan una cuenta real. La compactación general de IDs, autores compartidos y compresión opcional quedan condicionadas a evidencia adicional.

Como antecedente, P1 separó lectura y escritura del Gateway, reservó presupuesto para su salud, acotó reconexiones y espera de comandos, y agrupó presencia y suscripciones. Reutiliza el catálogo completo de READY con consultas selectivas de respaldo, vincula miembros a una superficie visible y agrupa ACK de lectura. El indicador de escritura es opcional. Las mutaciones comparten un registro de duplicados limitado a la sesión, que conserva resultados inciertos; las credenciales pasan por un tipo secreto y los temporales de login tienen limpieza cancelable y recuperación limitada a recursos propios. Pasaron **437 pruebas offline, cero fallidas y dos excluidas por defecto**; estas dos pruebas nativas se ejecutaron además en tres rondas consecutivas correctas. Las pruebas y límites históricos se registran en [P1-VALIDACION.md](evidence/P1-VALIDACION.md).

La suite histórica P0 pasó **378 pruebas offline, cero fallidas y una excluida por defecto**, ejecutada además expresamente con éxito. Incluye 47 regresiones nuevas de seguridad/transporte y conserva las funciones UI4. Se usaron credenciales ficticias, loopback y temporales; no una cuenta real ni dispositivos. El [informe P0](evidence/P0-VALIDACION.md) distingue lo comprobado de lo pendiente. El modelo de autorización de plataforma sigue pendiente según la [matriz de acceso](docs/MATRIZ_DE_ACCESO_P0.md).

El pase UI4 registrado tiene **331 pruebas correctas, cero fallidas y una excluida por defecto**: aplicación 255, núcleo 46 y addon 30, en `evidence/ui4-workspace-tests.txt`. Cubre interacción, scroll con snapshots demorados, navegación, reenvíos, lectura/ACK, imágenes/avisos y estados multimedia. El paquete verificó 16 binarios nativos y siete comprobaciones del host exacto, sin inicializar dispositivos; el gate IPC excluido por defecto también se ejecutó y aprobó expresamente. La [arquitectura actual](docs/ARQUITECTURA_RUST.md) distingue cada comprobación y su alcance.

El visor nativo superó ocho casos de apertura/cierre en dos ciclos con PNG/JPEG sintéticos, una imagen corrupta, error visible/reintento y salida cero. Esto no reproduce el archivo adjunto exacto que provocó el cierre del usuario. El desenfoque nativo pasó los niveles 0/50/100: contraste 96→7→4, luminancia conservada y error RGB cero en texto/línea/imagen opacos; véase [BLUR-NATIVO.md](evidence/BLUR-NATIVO.md). Ambas pruebas usaron sólo ventanas y datos propios, sin tocar la instancia autenticada.

Como historia de UI3 se conservan sus **288 pruebas correctas**, el gate de IPC/host ejecutado expresamente, siete comprobaciones dentro de Electron, 16 binarios nativos y capturas Release sin cuenta. El probe anterior de vídeo verificó ABI/sink con un frame RGBA sintético, ACK y liberación de referencias; el capturador Y4M no produjo callback (`FAKE_VIDEO_CALLBACK_TIMEOUT`). Sus resultados no acreditan cámara física ni vídeo remoto y no se convierten en validaciones UI4.

La preview nativa de una ventana sintética propia se comprobó de extremo a extremo (BMP a RGBA, ACK y limpieza). Siguen pendientes una llamada audible, cámara física y recepción/emisión de cámara o Go Live con otro cliente. El inicio autenticado con datos compatibles corresponde a la entrega inicial. El intercambio de portapapeles con otras aplicaciones sigue pendiente por permisos Windows, aunque sus conversiones PNG/HBITMAP se probaron con imágenes sintéticas. Las pruebas de datos/XML de avisos y sus límites están en [UI4-NOTIFICACIONES.md](evidence/UI4-NOTIFICACIONES.md); un test sintético no acredita recepción de mensajes reales.

La revisión de límites, vida útil de recursos y resultados de medición está en [MEMORIA_Y_RECURSOS.md](docs/MEMORIA_Y_RECURSOS.md). La comparación de RAM/CPU y tiempos en uso real corresponde al usuario; no se atribuye una mejora al cambio de lenguaje.
