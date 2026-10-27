# Plan de correcciones de interacción llamadas y GIFs de NotDiscord

Fecha: 27 de octubre de 2026, America/Lima. Estado: corte de planificación conservado; implementación 0.6.0 registrada en el seguimiento final. Skill utilizada: `notdiscord-planificar`.

El plan recoge los 22 puntos solicitados, incluidos menús modulares y el bloqueo al repetir clic derecho. Quedan 21 puntos activos: F12 se retiró por indicación del usuario tras hacer público el repositorio. Las prioridades son diagnosticar la caída de voz y el bloqueo sin ocultarlos, corregir los estados desactualizados y consolidar la interacción de teclado, menús y medios.

**Tamaño Mediana, elegido expresamente por el usuario.** Desde la base actual, la versión propuesta es **0.5.0 → 0.6.0**. No se ha cambiado el manifiesto. La cantidad de puntos no modifica esa elección; revalidar únicamente la versión de partida si cambia el checkout.

## Base alcance y restricciones

- Repositorio: `E:\DESCARGAS\REA\NotDiscordRust`; aplicación `NotDiscord-Rust/`; rama `NotDiscord`.
- HEAD: `b824e6340e716acae054daff6f57c6aacb49da0b`, manifiesto estándar `0.5.0`.
- Al iniciar, el producto estaba limpio y sólo había `NotDiscord-Rust/evidence/` sin seguimiento. Durante el análisis apareció un cambio ajeno en `skills/notdiscord-programar/references/release.md`; se conserva. No se cambia de rama ni se modifica código del producto en esta tarea.
- El usuario identifica `0.4.1` como la versión anterior para el problema de actualización. No confirmó si la caída de voz sucedió en esa misma versión o en otra. El mensaje de error existe tanto en las fuentes de 0.4.1 como en 0.5.0.
- NotDiscord+ permanece fuera de esta tanda; conservar su rama, instalación, datos y accesos.
- La petición del corte original era de planificación. La autorización posterior de implementación, compilación y publicación se registra en el seguimiento final; no autoriza ejecutar el cliente, entrar a cuentas, iniciar llamadas/dispositivos o provocar crashes reales. README y `docs/updates.md` mantienen la restricción de pruebas funcionales/cuentas. Las verificaciones de comportamiento de este plan son futuras y no se presentan como realizadas.
- Sí se efectuaron lecturas de fuentes/documentación y consultas HTTP de metadatos de distribución. No se descargó ni ejecutó el instalador. No se usaron cuentas de Discord ni se inspeccionaron chats, sesiones o memoria de cuentas.

Las rutas siguientes son relativas a `NotDiscord-Rust/`. `app/`, `core/` y `media-addon/` abrevian `crates/app/`, `crates/core/` y `crates/media-addon/`. Las líneas identifican el corte observado; los nombres de funciones permiten revalidarlas si cambian las fuentes.

El [plan anterior](plan-optimizacion-robustez-seguridad-2026-10-26.md), sección de seguimiento de 0.5.0, registra implementadas la vigilancia de heartbeat, las colas acotadas y la cancelación de voz. No se deben reimplementar como si faltaran ni atribuirles una regresión sólo por el nuevo aviso.

## Evidencia del usuario

La imagen 1 muestra un menú de mensaje con responder, reenviar, reacción, fijar, editar, eliminar y copia sin grupos semánticos. La imagen 2 muestra un menú de conversación con separadores entre conversación, perfil/llamada, notificaciones, acciones destructivas e identificadores. Es el patrón de organización solicitado, adaptado a cada tipo de objeto.

La imagen 3 muestra «Error de conexión de voz» y el texto «Falló la comunicación con el servidor de voz. Puedes volver a unirte para abrir una sesión nueva». El usuario lo observó al llegar una notificación mientras jugaba Outlast Trials. La coincidencia temporal es evidencia del incidente; no demuestra que el juego o la notificación sean su causa.

Los GIF duplicados, los botones desaparecidos al volver al chat y el bloqueo por clic derecho son observaciones del usuario. Las fuentes permiten identificar brechas y diseñar su investigación, pero no se reprodujeron esos tres incidentes. Se conserva esta distinción en cada apartado.

## Cobertura de los requisitos

«Confirmado» significa causa o brecha verificable en fuentes; «Mixto» combina esa evidencia con un incidente aún no reproducido; «Nuevo» identifica comportamiento que se añadirá.

| ID | Requisito y resultado esperado | Estado y bloque |
|---|---|---|
| F01 | Cambiar estado actualiza miembros, agrupación y demás superficies sin quedarse en el anterior | Confirmado; presencia y notificaciones |
| F02 | Reintentar un GIF reemplaza el estado fallido en el mismo slot; no deja otra sección antigua | Mixto; identidad de medios |
| F03 | Crashlogs y error logs locales, útiles, acotados y sin credenciales/contenido privado | Nuevo; diagnóstico |
| F04 | Investigar y corregir la salida de voz coincidente con notificación/juego | Mixto: ruta del aviso identificada, causa raíz pendiente; diagnóstico y voz |
| F05 | Ocultar volumen de transmisión cuando el participante no tenga una transmisión pertinente | Confirmado; voz |
| F06 | Reorganizar servidores mediante arrastre y conservar el orden | Nuevo; navegación |
| F07 | Evitar indicadores realmente obsoletos/falsos; mantener distinta la política de sonido | Mixto; presencia y notificaciones |
| F08 | Alt + teclado numérico, incluido Alt+64, introduce el símbolo esperado | Mixto; entrada nativa y teclado |
| F09 | Autoscroll con botón medio en todas las áreas desplazables, con hitboxes acordes al contenido | Mixto; geometría e interacción |
| F10 | Al terminar una transmisión, el visor sale de forma neutral y retira controles caducados | Confirmado en algunas rutas; voz |
| F11 | Las notificaciones Windows muestran menciones por nombre visible, no `<@id>` | Confirmado; presencia y notificaciones |
| F12 | Problema de actualización desde 0.4.1 | Retirado por el usuario tras hacer público el repositorio; sin fix planificado |
| F13 | Si hay pantalla y cámara, mostrar ambos botones apilados: pantalla arriba, cámara abajo | Confirmado orden inverso; voz |
| F14 | Volver al chat conserva botones de fuentes todavía anunciadas; comprobar DM/grupo/servidor | Reportado, mitigaciones existentes; voz |
| F15 | Mejorar apertura/carga de catálogo GIF y guardados con métricas comparables | Confirmadas ineficiencias; GIFs |
| F16 | Estrella activa en todos los guardados; al desmarcar quitar también el guardado de Discord | Confirmado y alcance remoto elegido por usuario; GIFs |
| F17 | Chat de llamada inicialmente visible en DM/grupos y oculto en servidores; preferencias independientes y persistentes | Confirmado incumplimiento; voz y preferencias |
| F18 | Autoenfoque de campos externos con prioridad sobre el compositor; sistema común | Confirmado incompleto; teclado |
| F19 | Ctrl+V enruta texto/imagen al campo válido del ámbito activo | Confirmado ligado al compositor; teclado |
| F20 | Enter confirma edición/acción apropiada; Shift+Enter inserta línea donde corresponda | Confirmado en edición; teclado |
| F21 | Menús contextuales agrupados por secciones mediante infraestructura modular | Confirmada inconsistencia; menús |
| F22 | Abrir clic derecho muchas veces rápido no deja la aplicación sin responder | Reportado; investigación prioritaria y menús |

## Diagnóstico y caída de voz

### Ruta concreta de la imagen 3

`media-addon/src/host.rs:2749–2756`, `socket_failure`, distingue HTTP, TLS, I/O y capacidad; la rama restante genera **`VOICE_SOCKET_ERROR`** y el texto de la captura. `media-addon/src/host/voice_socket.rs:102,130,159` puede recorrerla durante conexión, escritura o lectura. `Event::WsError` llega al actor de host y `voiceState error/disconnected`, en `app/src/controller/voice.rs:3077–3081`, termina la llamada y añade la indicación de volver a unirse. `ui/shell.rs:1992` dibuja el encabezado.

El texto no identifica una caída del proceso multimedia, un timeout de heartbeat ni un fallo TLS: esas rutas tienen otros códigos/textos. El catch-all puede reunir `Protocol`, `ConnectionClosed`, `AlreadyClosed`, `Utf8` y otros casos. Hay que conservar **fase, variante saneada, generación de sesión y motivo previo al cierre**, sin serializar el error WebSocket completo, frames, endpoints o credenciales.

La aparición de una notificación no ordena directamente salir de voz: `platform.rs:635–639` manda navegación al activarse el toast; el mero aviso y su sonido siguen rutas diferentes. Revisar mediante eventos correlacionados sonido, activación del toast, foco/minimización, voz y cierre remoto. No reiniciar audio, cambiar dispositivos o desactivar notificaciones para ocultar el síntoma.

Existe además otra pérdida diagnóstica: `app/src/media.rs:208` publica un evento `fatal` con `controlCode`, `exitCode` y `forcedStop`; `controller/voice.rs:3224` lo reduce a texto genérico. Debe registrarse también, pero **no es la ruta identificada por esta captura**.

**Aceptación F04:** un fallo sintético se clasifica por fase/variante y deja causa saneada antes del teardown. Un aviso o su activación no cierra voz ni reinicia la sesión multimedia. Una desconexión real informa de su clase sin exponer datos del servidor. La causa de Outlast Trials se cierra sólo con evidencia del incidente; la clasificación genérica actual no basta.

### Sistema común de crashlogs y error logs

En producción no se encontró un sistema unificado de persistencia de errores/panics. `main.rs:68` inicializa el cliente sin panic hook propio y en `195–198` ignora el resultado de esperar al controlador. `host.rs:284–287` sólo emite trazas opcionales de etapas a stderr; `media.rs` arranca el host con stderr descartado. Los logs de probes y del instalador no sustituyen los del cliente.

Crear un servicio `diagnostics` compartido por los adaptadores del cliente y host, con esquema versionado y propietario por ejecución. Eventos tipados: arranque/cierre, error recuperable, fallo de tarea, estado de transporte, pérdida de host, panic y ejecución anterior no cerrada ordenadamente. Guardar versión/build, timestamp UTC, contador monotónico, componente, código, fase, severidad y correlación efímera. No guardar texto del chat, portapapeles, tokens, cookies, headers, argumentos completos, URLs firmadas, nombres de usuario/juego o títulos de ventana.

Propuesta inicial de presupuestos, ajustable durante implementación: buffer circular de 256 eventos; entradas de hasta 4 KiB; archivos de hasta 1 MiB y cuota total de 16 MiB/14 días. Rotación por ejecución, retirada sólo de archivos propios inactivos, deduplicación de errores repetidos y escritura ordinaria fuera del hilo UI. Los fallos de logging no deben bloquear un menú, causar otra desconexión ni llenar una cola ilimitada. Una pequeña vía de emergencia para panic debe ser reentrante segura y no esperar un lock retenido por el hilo que falla.

Instalar el hook antes de inicializar tareas/UI y registrar panics de hilos con ubicación saneada y backtrace controlada cuando sea viable. No imprimir el payload arbitrario de panic: puede incluir datos. Un hook registra un panic; `catch_unwind` sólo captura unwinding y no convierte cualquier crash nativo en un error recuperable. [Contrato de panic hook](https://doc.rust-lang.org/std/panic/fn.set_hook.html), [límites de catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).

Cubrir salida inesperada del host, `JoinError` de tareas supervisadas y un marcador de cierre ordenado. Un marcador incompleto sólo significa «terminación no limpia», no prueba crash: puede deberse a apagado o terminación externa. Para excepciones nativas, registrar desde el padre el estado disponible. Un volcado `.dmp` no equivale a un log textual; quedaría como diagnóstico avanzado optativo, con alcance y privacidad explícitos, no como captura automática de memoria de una cuenta. [Microsoft recomienda separar la escritura de minidumps del proceso inestable](https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/nf-minidumpapiset-minidumpwritedump).

UI mínima: abrir carpeta de diagnóstico, ver resumen de errores y exportar voluntariamente un paquete saneado. No habrá envío automático. El paquete de distribución debe excluir logs, marcadores locales y exports. Preservar símbolos/build identifiers para interpretar los errores de una release sin incluir símbolos privados ni datos en el instalador por accidente.

**Aceptación F03:** error recuperable no se etiqueta crash; panic/tarea/host tienen registros distinguibles; cuota/rotación funcionan ante ráfagas y disco lleno; un fallo del logger no bloquea el cliente; datos sintéticos sensibles no aparecen en logs/export. Las pruebas de panic sólo se proponen en procesos auxiliares sintéticos, nunca provocando un crash de la instancia del usuario.

## Menús modulares y bloqueo al repetir clic derecho

`app/src/ui/chat/context_menu.rs:99–139` ya concentra el popup de una fila de mensaje; los hijos aportan `Target::Link/Media/Person`. Conservar este contrato. `message_menu_with_text`, en `chat.rs:1342–1462`, y `media_menu`, en `1737`, juntan acciones sin secciones internas. `nav_menu`, en `shell.rs:1791`, sí usa varios separadores y sirve de referencia semántica.

Crear un módulo común de menús con helpers de sección, acción normal, toggle, opción deshabilitada con explicación, submenú y acción destructiva. Separar obtención de contexto/datos, presentación y ejecución de la opción elegida. Las secciones vacías no dejan separadores; no hay separador inicial, final o doble. No convertirlo en un registro global que ejecute trabajos al dibujar.

Grupos propuestos, según el objeto:

- Mensaje: responder/reenviar/reaccionar; fijar/editar si corresponde; eliminación y sus confirmaciones; copiar texto/enlace/identificador.
- Medio/enlace: reproducción/reintento; abrir/guardar; portapapeles. Mantener la separación con las acciones del mensaje padre sin duplicarlas.
- Conversación/servidor/canal: navegación y estado de lectura; perfil/comunicación; preferencias/notificaciones; moderación o acciones destructivas; identificadores.
- Participante/visor: comunicación; audio del participante; transmisión pertinente; moderación; identificadores. Los grupos inexistentes se omiten.

Migrar mensajes, imágenes/GIFs, enlaces, personas, compositor, rail, servidores, canales, DM, cuenta y participantes/visor. El menú `⋯` debe producir las mismas acciones que clic derecho, con el mismo target, permisos, orden y estado.

**F22 sigue sin causa demostrada.** Las transacciones `data_mut` inspeccionadas en `Scope::begin/Drop` del adaptador no mostraron la reentrada que causó el antiguo bloqueo de arranque. No atribuir el freeze a los separadores ni prometer que el refactor visual lo arregla. Revisar durante la investigación todos los handlers que puedan bloquear o entrar al mismo `Context` dentro de `data_mut`, `input`, `memory` y equivalentes; obtener metadatos y destruir guards fuera de esas transacciones.

Hay una ruta de demanda al abrir menús de personas: `people_menu` → `draw_member_moderation_menu`, `ui/moderation.rs:39–64`. Deduplica el contexto actual, pero alternar usuarios puede cambiarlo y generar solicitudes. Instrumentar aperturas efectivas, demandas, popups retenidos, longitud de colas y latencia de frame; no afirmar que esa demanda ya explica el bloqueo. Abrir un menú no debe ejecutar la acción de un elemento ni esperar una respuesta de red.

**Aceptación F21/F22:** misma fila y targets alternos, clic derecho repetido, Escape, clic exterior, submenús, navegación, borrado del objeto y cambio de cuenta no dejan propietarios obsoletos, crecimiento sin límite ni doble acción. El target queda fijado al abrir y una respuesta tardía no lo reemplaza. Proponer una fixture de ráfagas con plazo desde un proceso separado para detectar deadlock sin colgar la suite. No usar un debounce arbitrario como sustituto de la causa.

## Teclado foco portapapeles y autoscroll

### Propiedad del teclado por ámbito

La detección principal está especializada en el compositor (`ui/chat.rs:1842–1857`) y la búsqueda (`ui/dialogs.rs:1455–1502`). `bordered`/`BorderedTextEdit`, en `ui/mod.rs:98–125`, puede ser el adaptador común de presentación, acompañado de un servicio de foco y entrada que seleccione propietario **antes** de procesar el texto.

Cada campo registra ID estable, ámbito/ventana, función, prioridad, validación y capacidades. Orden: campo ya elegido en el ámbito superior activo; campo principal al abrir un modal/popup con texto; campo elegido mediante Tab/clic; compositor principal como fallback. Cualquier textbox externo del ámbito activo prevalece sobre el compositor. Dentro de ese ámbito no se roba el foco al segundo campo que el usuario eligió. Un popup sin campo, diálogo nativo, arrastre o ventana no foreground impide que la entrada se filtre al compositor.

No pedir foco en cada frame. Transferir el primer carácter una sola vez, respetar orden de eventos, cerrar/restaurar superficies y composición IME. El shell se dibuja antes que diálogos (`ui/mod.rs:1663–1673`); resolver el propietario sólo después de dibujar todos los campos llegaría tarde.

Inventario mínimo: compositor; editor de mensaje/prompts; búsqueda principal y reenvío; catálogo GIF/emojis; perfil/estado/biografía; apariencia; servidor/roles/canales/apodo y filtros de listas. La política depende de función, no de copiar el handler del compositor en todos ellos.

### Enter Shift Enter y pegado

`draw_edit_prompt`, `ui/dialogs.rs:953–955`, usa `TextEdit::multiline` sin acción de Enter; sólo el botón en `979–990` envía el cambio. Reutilizar los contratos de Enter/IME ya existentes (`prepare_composer_input`, `composer_send_enter`, `consume_composer_send_enter`, `composer_ime_active`, en `chat.rs:2593–2637`) adaptados al editor dueño.

- Mensajes/edición: Enter confirma una sola vez, con la misma validación y revisión que el botón; Shift+Enter inserta línea.
- Búsquedas: Enter busca/acepta la sugerencia pertinente, sin ejecutar una acción destructiva ni enviar al chat detrás.
- Formularios de una línea: Enter activa la acción primaria válida del ámbito. No saltarse confirmaciones de moderación.
- Biografía/descripciones largas: conservar Enter como texto cuando esa sea la función natural. No generalizar «Enter guarda» indiscriminadamente.

Ctrl+V debe funcionar aunque el campo válido aún no tenga foco. La interceptación de imágenes está hoy ligada al compositor (`chat.rs:1934–1967`) y retira `Event::Paste` si el clipboard contiene imagen, aun si también hay texto. Crear un enrutador de capacidades: todos los campos de texto aceptan texto; compositor y superficies de edición que realmente admitan adjuntos aceptan multimedia; filtros, apodos y búsquedas nunca generan adjuntos. No añadir capacidad de modificar adjuntos a un editor que no la tenga.

Resolver clipboard mixto mediante política explícita del campo y entregar una sola representación, sin doble pegado ni pérdida silenciosa de texto. Mantener antirrebote, foreground, tamaño de adjuntos, cuota de texto, cuenta/canal/generación y cancelación. Conservar los helpers de `ui/text_budget.rs`, `paste_gesture` y el manejo del texto que llega después de Enter dentro del mismo lote de eventos.

### Alt y códigos de símbolos

No corregir únicamente `@`. `composer_printable_input`, `chat.rs:2587–2590`, rechaza Alt solo: puede afectar autoenfoque, pero no explica por sí mismo la pérdida con un editor ya enfocado. Hay una hipótesis de integración Windows en las dependencias efectivas `winit 0.30.13` y `egui-winit 0.33.3`: caracteres sin `event_info` pendiente pueden descartarse y el puente entrega texto sólo para pulsaciones. La llegada de Alt+numpad al soltar Alt necesita verificación nativa.

Windows distingue teclas físicas y mensajes de caracteres y no garantiza relación uno a uno. [Contrato de WM_CHAR](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-char). Proponer observación de eventos únicamente de la ventana propia con texto sintético; elegir corrección de dependencia o puente local de caracteres confirmados si se demuestra la brecha. No implantar un hook global ni un acumulador de dígitos por polling. Conservar UTF-16/surrogates, AltGr, teclas muertas, IME, Alt+Tab/Alt+F4 y evitar texto duplicado.

### Rectángulos interactivos y botón medio

`ui/theme.rs:137–146` aplica manita a cualquier respuesta clickable sin drag, incluida la fila completa de mensaje (`chat.rs:607–612`). Sin embargo, las filas se registran como excepciones permitidas para autoscroll (`history_selection_finish`, línea 2875; `ui/autoscroll.rs:83–92`). La manita amplia no es por sí sola la causa de todos los fallos.

Sí hay un desajuste concreto: `media_card`, `chat.rs:1623–1644`, reserva un rectángulo por metadata/fallback y puede pintar una imagen menor en `image_rect`; la interacción continúa sobre todo el rectángulo reservado (`1668–1681`). Separar área de layout y área interactiva: imagen lista usa su rectángulo pintado; placeholder usa el área visible de la tarjeta; enlaces multilínea usan rectángulos por línea del galley. Recortar por viewport/capa. Mantener el tamaño completo de botones, chips, toggles y filas cuya función sea seleccionar, según lo pedido.

Reutilizar `autoscroll::drive/unclaimed/exclude` y el registro de selección; el cursor no debe ser el criterio de bloqueo. Revisar chat/historial/pins/búsqueda, servidores/canales/miembros, catálogo GIF, listas de llamadas, diálogos y ajustes. Popups, controles y enlaces reales siguen tomando sus gestos; el espacio libre permite botón medio. Evitar dos propietarios en áreas de scroll anidadas y conservar cancelación por Escape/clic/rueda y pérdida de foco.

**Aceptación F08/F09/F18/F19/F20:** primer carácter y Ctrl+V llegan sólo al campo superior válido; Tab/clic mantiene otra elección; Enter/Shift+Enter e IME no duplican solicitudes; Alt+64 produce un único `@` también en editores modales. Probar texto, bitmap, mixto, vacío y Shift+Insert por capacidades. El botón medio funciona sobre fondos y espacio sobrante sin abrir enlaces, arrastrar texto ni activar botones. Inyectar `Event::Text("@")` no demuestra soporte nativo de Alt+numpad.

## Voz transmisión cámara y chat de llamada

`draw_call_participant_menu`, `ui/voice.rs:1573`, dibuja el volumen de transmisión incondicionalmente; `draw_stream_volume_control` siempre habilita el slider. Introducir un predicado compartido por participante/fuente para menú de tile y visor ampliado: no mostrarlo para el usuario propio ni para alguien sin transmisión remota anunciada pertinente. Que desaparezca el control no obliga a borrar la preferencia de volumen guardada.

`draw_media_participant_tiles` ya usa disposición vertical; la iteración actual (`voice.rs:1457`) dibuja cámara antes que pantalla y calcula ese orden en la altura del footer (`1357`). Cambiar ambos a **pantalla arriba, cámara abajo**, mantener ambos cuando existan y aplicar el componente compartido a DM, grupos, servidores y demás vistas que lo reutilicen.

`STREAM_DELETE`, `controller/voice.rs:2529–2546`, envía varias terminaciones normales a `fail_watch`; éste repone placeholder de error (`2785–2799`). `watchState` también mezcla `disconnected` y `error` (`3030–3045`). En contraste, `VOICE_STATE self_stream=false` limpia neutralmente. Definir estados distintos para fin del emisor, cancelación del espectador, pausa por visibilidad, rechazo de acceso/cupo y error de decoder/transporte. El fin normal vuelve al avatar/estado disponible pertinente y retira controles terminados, conservando cámara o audio que sigan activos.

El fallo al salir del chat y volver requiere investigación: ya existe pausa por visibilidad (`ui/mod.rs:1580–1604`) y preservación de selección al pasar `paused → waiting` (`controller/voice.rs:2944–2952`). Separar cuatro estados: fuente anunciada por el emisor, elección del espectador, visibilidad de la ventana y ciclo del decoder. Pausar recepción no elimina el anuncio ni la elección; una nueva generación de llamada sí invalida fuentes antiguas. Los botones dependen de anuncios vigentes, no de que sobreviva una textura del frame anterior.

Para F17, `call_chat_hidden`, `voice.rs:10–23`, oculta por defecto todos los tipos de llamada y conserva un único valor temporal `(scope, shown)` que incluye `call_instance_id`. Sustituirlo por **dos preferencias persistentes independientes por cuenta**: llamadas privadas/grupos, inicialmente visible; servidores, inicialmente oculto. El intento de llamada no forma parte de la identidad de esa preferencia. Guardar la elección explícita y conservarla al salir/volver a llamar. No introducir preferencias por cada canal como requisito adicional sin necesidad; la separación mínima solicitada es privados/grupos frente a servidores.

**Aceptación F05/F10/F13/F14/F17:** matriz DM/grupo/servidor con sólo pantalla, sólo cámara, ambas y ninguna; iniciar, alternar, navegar a otro chat, volver, minimizar/restaurar, terminar desde emisor/espectador y eventos tardíos. Pantalla/cámara mantienen orden y controles; fin normal no deja error; volumen sólo en fuente pertinente; audio no se reconecta por cambiar vista; las dos preferencias de chat sobreviven reinicio y no se contaminan entre ámbitos/cuentas.

## GIFs identidad favoritos y carga del catálogo

### Un medio lógico y un slot de presentación

`media_card`, `chat.rs:1614–1666`, ya pinta placeholder o textura en un único rectángulo. Reintentar (`chat/context_menu.rs:155–158`) sólo limpia errores de caché: no se confirmó que ese botón cree por sí solo otra sección.

Sí existe una brecha alcanzable: `message_inline`, `chat.rs:1294–1301`, excluye URL de texto si coincide con `attachment.url` o `preview_url`, pero no con `animation_url`. `core/src/parsing.rs:590–603` puede conservar página, poster y rendition separados. Un rendition en el texto puede generar una tarjeta inline y otra del attachment, cada una con estado de carga distinto. La deduplicación del parser y la del render no usan el mismo contrato.

Crear un descriptor compartido con identidad lógica, página pública, poster, rendition, dimensiones y aliases acreditados por metadata; reutilizarlo en mensaje, catálogo, favorito y visor. Mantener separado el identificador de medio de la URL de descarga firmada. Un slot por mensaje/medio pasa por loading/ready/error; retry cambia intento/fuente sin añadir otro slot y descarta resultados de intentos anteriores. No unir medios por títulos parecidos ni eliminar arbitrariamente query strings de proveedores. Preservar selección, ancla de scroll y otros medios legítimos del mensaje.

### Guardados efectivos y eliminación también en Discord

El usuario confirmó expresamente que desmarcar un GIF remoto debe quitarlo **también de Discord**. `pickers.rs:102,121–123` lee favoritos remotos mediante GET `users/@me/settings-proto/2`, pero la estrella/toggle (`ui/dialogs.rs:2012–2040`) sólo consulta/muta la lista local. `1958–1967` inserta favoritos locales sobre `picker.items` sin separar la categoría anterior; quitar un local tampoco reconstruye inmediatamente esos items.

Unificar identidad y estado efectivo de guardado. Retirar un remoto requiere confirmar el cambio remoto y eliminar además su copia local para que la fusión no lo resucite. Una operación local no se presentará como eliminación de Discord. Conservar el comportamiento de alta existente mientras se concreta el contrato de sincronización; no hacer escrituras masivas de todos los ajustes por una estrella.

Si Discord confirma la eliminación y después falla el guardado local, representar «eliminado en Discord; limpieza local pendiente». Reconciliar/reintentar sólo la limpieza local y evitar que el arranque o la fusión resuciten la entrada. No restaurar el favorito remoto como compensación ni repetir su eliminación a ciegas. Preparar el estado de reconciliación de forma durable cuando sea posible; si esa preparación falla, no empezar una mutación que después no pueda conciliarse.

Hay dos representaciones de historial (`ui/mod.rs:228–241` y `picker_history.rs`); la UI evita la ruta transaccional y `save_picker_history`, `dialogs.rs:1773–1778`, ignora errores. Elegir un propietario por cuenta y una operación que persista el candidato antes de confirmar el estado visual o revierta correctamente una presentación provisional. Los errores deben ser visibles: preservar el último estado confirmado de cada origen, distinguiendo fallo remoto de éxito remoto con limpieza local pendiente.

La vista `GifPage/GifItem` **no es un formato de escritura seguro** de settings: `pickers.rs:187–260` descarta campos desconocidos y entradas que no presenta. `settings_proto.rs` opera con ajustes tipo 1, no implementa favoritos tipo 2. El PATCH tipo 1 de `controller/social.rs:637–658` es un precedente de transporte, no prueba la semántica de borrado o combinación del mapa tipo 2.

Antes de programar la mutación remota, verificar el contrato real del subárbol tipo 2: representación de eliminación, merge/reemplazo, revisión/conflicto y `out_of_date`. Usar transformación protobuf que preserve campos/bytes desconocidos, otros favoritos y metadatos; no serializar de vuelta la vista filtrada. No probar payloads adivinados contra una cuenta. La lectura de artefactos/documentación autorizados puede resolver ese esquema; si no hay evidencia suficiente, ese tramo queda pendiente sin bloquear los demás fixes.

Enviar intención idempotente `saved=false` con cuenta/generación/identidad, no «toggle» reintentado a ciegas. Serializar/coalescer operaciones propias y reconciliar con lectura posterior; eso no garantiza atomicidad frente a otros clientes. Ante timeout tras envío, consultar estado antes de otra escritura. Mantener corte de seguridad, rate limits y rechazo de mutaciones; registrar la acción en `is_network_mutation`. Manejar `USER_SETTINGS_PROTO_UPDATE` tipo 2: hoy `incoming.rs:792–805` sólo consume tipo 1.

### Catálogo y respuestas asíncronas

La caché actual conserva cuatro endpoints durante 60 segundos y expulsa arbitrariamente por `HashMap.keys().next()` (`controller.rs:1843–1848,2230–2236`). «Cargar más» solicita prefijos 20→50→96 y el endpoint redondea a 100. El grid ya está virtualizado y solicita thumbnails visibles; conservarlo.

Separar resultados por modo/consulta/categoría y favoritos. Reconstruir por revisión en lugar de buscar e insertar linealmente cada favorito en cada frame. Presentar locales disponibles inmediatamente; LRU acotada por entradas/bytes, deduplicación de peticiones en vuelo y reutilización de un resultado mayor compatible. Preservar rendition/dimensiones al guardar para no resolver de nuevo cada página después de reiniciar.

`UiUpdate::Data("gifs")`, en `ui/mod.rs:1093–1108`, no comprueba tipo del picker abierto, generación de apertura, consulta o modo. Un resultado GIF puede aterrizar en otra apertura, incluso emojis. Los errores no terminan `picker.pending` por la misma ruta de éxito. Añadir correlación completa y cancelación: cuenta/generación, apertura, modo, consulta e ID de petición. Éxito, error y cancelación finalizan pending sólo para su operación; respuestas antiguas no reemplazan otro catálogo.

**Aceptación F02/F15/F16:** fallo→retry→éxito conserva una tarjeta; alias página/poster/rendition no duplica y otro medio distinto permanece. Guardados muestra sólo sus items, estrella activa y retirada inmediata coherente con persistencia; quitar remoto se verifica en Discord mediante resultado canónico, sin reintroducirlo localmente. Cerrar/reabrir, cambiar categoría, abrir emojis, fallo, cancelación o cambio de cuenta no deja spinner ni aplica respuesta antigua. Fixtures protobuf preservan campos desconocidos y cambios ajenos. Medir tiempo a primer thumbnail/grid, peticiones/bytes, resoluciones, hits/misses y tiempo de frame en frío/caliente, sin anunciar ahorro antes de medir.

## Presencia indicadores y nombres en notificaciones

`apply_self_presence`, `app/src/controller/presence.rs:29–41`, actualiza `self_presence` y la barrera DND, pero no la presencia de miembros/mapa `user_presences`. `person_row_inner`, `ui/shell.rs:2277–2278`, prioriza ese mapa antiguo también para el usuario propio; `member_group`, `277–287`, agrupa por la presencia del miembro. Una elección confirmada puede verse en el footer y seguir antigua en la barra.

Usar una presencia efectiva compartida: para uno mismo, estado confirmado local; para otros, el dato vigente del ámbito. Aplicarla a avatar, texto, agrupación, perfil y llamadas sin borrar custom status/actividades. Conservar barrera de notificaciones y tratar eventos retrasados para que no reviertan un cambio confirmado.

Los badges y sonidos son contratos diferentes: `incoming.rs:347–377` marca no leído aunque `delivery.notify` sea falso o DND/mute suprima el sonido. Eso puede ser legítimo; **no quitar todos los indicadores que no sonaron**. Hay una retención concreta que investigar: `refresh_read_guild`, `1027–1093`, depende del catálogo seleccionado y conserva unread si faltan permisos/canales/watermarks. Un ACK de otro servidor puede no localizar el guild o no poder reconciliarlo.

Mantener estado mínimo por canal/servidor y reconciliar con watermarks confirmados, acceso y catálogo de su propio ámbito. Un ACK viejo no borra una llegada nueva; información incompleta no se convierte en «todo leído». Registrar sólo contadores/clases y correlaciones efímeras necesarias para diferenciar badge legítimo, obsoleto y sonido suprimido, sin IDs de cuenta/chat persistidos en logs.

`message_notice_body`, `incoming.rs:62–70`, devuelve contenido crudo truncado a 50 caracteres; de ahí los `<@id>` de Windows. Crear un resolvedor puro de presentación con contexto explícito del mensaje de origen, metadatos de menciones, nicknames/caché del guild correcto y fallback neutro. Resolver **antes** de truncar, sin consultas por cada toast. No reutilizar directamente `ui/mentions::readable`, que usa el guild actualmente seleccionado. Mantener literales de código/escapes, Unicode, nombres de rol/canal cuando corresponda y XML escaping existente en `platform.rs`.

**Aceptación F01/F07/F11:** cambio propio confirmado actualiza fila/grupo sin esperar eco remoto; estados de otros siguen actualizándose. ACK de otro servidor reconcilia sólo ese ámbito con información suficiente, sin borrar mensajes más nuevos. DND/mute/sólo menciones conservan sonido previsto. Un toast de un guild no seleccionado usa sus nombres de origen; desconocidos, `<@id>`, `<@!id>`, nicknames diferentes por guild y truncamiento no muestran código crudo ni toman datos de otro servidor.

## Reordenar servidores por arrastre

`ui/shell.rs:900–918` dibuja el rail mediante `show_rows` de 56 píxeles e interacción de clic/menú, sin contrato de drag-and-drop de orden. Añadir orden local persistente por cuenta mediante el almacenamiento compartido actual. El pedido no requiere cambiar ajustes remotos de Discord ni crear/carpetizar servidores.

Arrastrar por ID estable, con umbral de movimiento, indicador de inserción, autoavance del scroll al borde y cancelación por Escape/fuera de ámbito. Resolver índices respecto al listado completo, no sólo filas visibles. Un drag no navega ni abre menú; un clic que no superó el umbral conserva navegación. Home/DM y otros elementos fuera de la lista de servidores mantienen su función. Actualizaciones de snapshot, alta/baja de guild o pérdida de permiso durante el gesto no cambian el target ni escriben índices obsoletos.

Persistir una lista validada de IDs, no un snapshot completo de servidores: deduplicar, retirar ausentes y añadir nuevos en posición determinista. Ante fallo de guardado, informar y conservar/restaurar el último orden confirmado. Evitar que cada snapshot reponga el orden recibido sobre la elección local.

**Aceptación F06:** arrastre arriba/abajo, entre filas no visibles, rail largo y bordes; clic/menú intactos; Escape cancela; orden sobrevive navegación y reinicio, sigue ligado a la cuenta y acepta cambios de membresía sin duplicar/perder guilds.

## Actualización retirada del alcance

El usuario informó que hizo público el repositorio y pidió eliminar el fix de actualización y no seguir verificándolo. **F12 queda retirado de esta tanda por esa indicación.** Se eliminan la propuesta de otro canal, cambios de endpoints/validadores, instalación inicial manual y cambios de UI del actualizador. No queda trabajo de corrección o comprobación de actualización asignado a esta tanda. Esto registra una decisión de alcance, no una prueba funcional del actualizador.

## Tandas de implementación y propiedad de componentes

| Tanda | Objetivo y requisitos | Dependencias y aceptación principal |
|---|---|---|
| T0 Diagnóstico y aislamiento | F03/F04/F22: logging acotado, clasificación de WebSocket y reproducción sintética propuesta del freeze | Corte vigente; conservar causas antes del teardown; no refactor amplio que oculte el freeze |
| T1 Menús comunes | F21 y corrección de F22 cuando su causa esté probada; módulos y migración progresiva de grupos | T0 para trazas; único propietario/target, acciones y permisos conservados |
| T2 Entrada e interacción | F08/F09/F18/F19/F20: árbitro de foco, políticas de campos, pegado, geometría y Alt | Coordinar con T1; diagnóstico nativo Alt pendiente; sin click-through, pérdidas ni doble acción |
| T3 Voz y preferencias | F05/F10/F13/F14/F17: fuentes/visor/control de volumen/orden de botones y dos preferencias persistentes | T0 y adaptadores de T1; matriz DM/grupo/servidor con audio preservado |
| T4 GIFs | F02/F15/F16: descriptor lógico, slots de retry, catálogo correlacionado, historial único y retirada remota | T1/T2 para UI; evidencia de contrato tipo 2 antes de su mutación; cache/carga acotadas |
| T5 Presencia y notificaciones | F01/F07/F11: presencia efectiva, read states por ámbito y resolvedor puro del toast | T0 para observar; preferencias DND y barreras de generación conservadas |
| T6 Orden de servidores | F06: gesto drag, inserción y persistencia local por cuenta | Infraestructura de interacción T1/T2; snapshots no sobrescriben orden |
| T7 Integración y entrega | Integración de los 21 requisitos activos para la tanda 0.6.0; F12 excluido | Checks permitidos y flujo de entrega existente; publicación/instalación sólo según pedido posterior |

T0 diagnóstico e investigación de T4/T5 pueden avanzar en paralelo. `ui/chat.rs`, `ui/dialogs.rs`, `ui/mod.rs` y `controller/incoming.rs` tienen cambios compartidos: asignar un propietario de integración por archivo y dividir helpers nuevos en módulos para evitar ediciones simultáneas incompatibles. T1 no debe convertirse en un refactor masivo de todo el UI antes de poder revisar una corrección concreta.

## Verificación datos y entrega

No se ejecutaron pruebas, compilación, cliente, cuentas, dispositivos, llamadas o instaladores. La comprobación de HTTP de distribución sí se realizó y está distinguida de una prueba funcional del actualizador.

Para programación, seleccionar pruebas puras/sintéticas que no inicien clientes, host multimedia, audio, cuentas, sockets externos o instaladores; las restricciones funcionales siguen vigentes. Una prueba de la política de teclado no sustituye una prueba Windows de entrada Alt. Un fixture de stream no demuestra reproducción real de vídeo. Una compilación no acredita que ya se arregló el freeze.

| Área | Casos futuros imprescindibles |
|---|---|
| Menús | Ráfaga mismo/otro target, submenú, Escape/clic exterior, borrado remoto, permisos y cambio de cuenta; timeout externo para deadlock |
| Teclado | Primer carácter, Tab, Enter repetido, Shift+Enter, IME en composición/commit, AltGr/códigos Alt y clipboard por capacidades |
| Geometría | Texto corto/multilínea, enlace y espacios, imagen pequeña/placeholder, botón completo, scroll anidado, popup y arrastre |
| Voz | DM/grupo/guild, cámara/pantalla/ambas, navegar/minimizar/volver, fin normal/error/pausa y eventos tardíos; audio no reiniciado |
| GIF | Doble fuente del mismo GIF, retry viejo, categoría previa, respuestas GIF→emoji, fallo/cancelación, favorito local/remoto y persistencia fallida |
| Settings remotos | Protobuf desconocido preservado, eliminación precisa, conflicto externo, 429, timeout tras envío, cambio de cuenta; sin escrituras de payload no verificado |
| Presencia y lectura | Confirmación propia sin eco, evento atrasado, ACK viejo/nuevo y otro guild, permisos parciales, silencio legítimo, menciones fuera de canal actual |
| Diagnóstico | Panic/tarea/host/error ordinario, ráfaga, cuota, disco lleno, fallo del logger, secretos sintéticos excluidos y export voluntario |

Persistencia: añadir esquema y defaults compatibles para orden y chat de llamada mediante el almacén actual. No crear carpetas de configuración alternativas ni mezclar preferencias entre cuentas. Mantener migraciones y marcadores de seguridad. Para favoritos, preservar URLs originales/identidad y datos desconocidos; no borrar historial ajeno por limpiar un favorito. Una reversión de la aplicación no debe escribir automáticamente un snapshot remoto antiguo y resucitar favoritos eliminados.

Rendimiento: comparar frío/caliente y visible/segundo plano con mismo contenido y mismas métricas: tiempo a primer resultado, frame mediano/p95, peticiones y bytes, colas, CPU, commit y working set; GPU separada. La infraestructura nueva de logging/menús/foco debe tener memoria y trabajo acotados. No imponer el número bajo de RAM de una captura como meta sin equivalencia de escenario.

Entregable actual: este plan dentro del proyecto. Entrega futura propuesta: cambios revisables por tanda, estado de requisitos, validaciones y limitaciones, versión Mediana 0.6.0 desde la base actual y, cuando se autorice, instalador y publicación mediante el flujo existente. La planificación no ejecuta esa publicación ni cambia visibilidad de repositorios.

## Estado y decisiones abiertas

- [x] Mapear los 22 requisitos y las tres capturas a componentes compartidos.
- [x] Localizar el error de la imagen 3 en `VOICE_SOCKET_ERROR`, también presente en 0.4.1.
- [x] Retirar F12 y su fix de actualización por indicación posterior del usuario; sin más verificaciones de ese punto.
- [x] Registrar tamaño Mediana y eliminación de favoritos también en Discord por decisión del usuario.
- [x] Revisar el plan con agentes independientes y corregir la ruta de voz y la reconciliación de éxito remoto con fallo de persistencia local.
- [ ] Causa raíz de caída de voz y freeze por clic derecho: diagnóstico/reproducción pendientes, sin atribución al juego ni al separador visual.
- [ ] Confirmar versión ejecutada en el incidente de llamada si hace falta distinguir código de binario instalado.
- [ ] Establecer con evidencia el contrato de eliminación/conflicto de favoritos settings tipo 2 antes de enviar una mutación.
- [ ] Implementar y comprobar las tandas sólo cuando se solicite programación; actualizar este estado sin repetir trabajo ya terminado.

Huellas de referencia: `app/src/updates.rs` SHA-256 `6cc845a9a8825dd010756bf5816abdafb6e95ba6c3ebdb8c1f83ae4e10b87aca`; `app/src/ui/chat/context_menu.rs` `fa145b68f68040da8df075e4c6270fa0c8f8cee034e74daa2d3f7ee5f955cc37`; `media-addon/src/host.rs` `a05781b2c0af81a28ee3f921c92eba1d940a2e07b3061ef3d73095d3a0da0cf9`. Revalidar funciones afectadas y baseline al continuar.

## Seguimiento de programación autorizado — 0.6.0

El usuario pidió ejecutar este plan y confirmó compilar y publicar el instalador al terminar. Se conserva su clasificación Mediana y la base 0.5.0; entrega prevista 0.6.0 estándar, sin modificar Plus. Las restricciones de no usar cuentas ni ejecutar pruebas funcionales, cliente o instalador continúan. F12 permanece retirado.

La planificación original se conserva como corte de evidencia. Este seguimiento registra la implementación, revisiones estáticas, comprobaciones de compilación y las limitaciones que permanezcan; no sustituye la confirmación funcional del usuario.
### Estado integrado y revisión de las tandas

| Requisitos | Cambios implementados y límites |
|---|---|
| F01 | La presencia propia confirmada se proyecta sobre mapa, filas, grupos y perfil sin borrar actividad/custom status. Los settings remotos que contradicen una elección confirmada se concilian mediante una única lectura canónica correlacionada, con coalescing; una nueva elección invalida respuestas anteriores. Los errores conservan el último confirmado y liberan el estado pendiente. |
| F02 | Descriptor de medio común con página/poster/rendition y aliases acreditados; inline y adjuntos comparten identidad. Retry conserva el slot; no se eliminan queries firmadas ni se unen dos páginas sólo por un poster. |
| F03 | Servicio tipado con ring/cola acotados, escritor independiente, panic sin payload arbitrario, supervisión de tareas y marcador de cierre. Resumen, carpeta y export voluntario en UI. Detalles y límites en diagnostics-0.6.0.md; sin minidumps ni envío automático. |
| F04 | Clasificación fija de errores WebSocket por fase/variante antes del teardown, causa de control/exit/forcedStop del host, y eventos agregados de sonido/toast/foco/minimización. El incidente original no dejó datos suficientes: causa causal de esa caída aún pendiente, sin atribuirla al juego o al aviso. |
| F05 | Volumen de transmisiones sólo para una pantalla remota anunciada pertinente; no aparece en uno mismo ni en un participante sólo de voz/cámara. La preferencia global guardada se conserva. |
| F06 | Orden local por cuenta mediante arrastre por ID estable en rail virtualizado, inserción/bordes/cancelación y escritura estricta previa a confirmar. Se relee navegación y se preservan mapas de otras cuentas/campos. |
| F07 | Índice limitado por guild/canal conserva horizonte y cobertura acreditada para ACK fuera de la vista actual. Invalidaciones de permisos/catálogo, ACK viejos y datos incompletos no se convierten en «todo leído». Los canales privados no se atribuyen al guild seleccionado. Sonidos/DND mantienen contratos separados. |
| F08 | Parches emparejados de winit 0.30.13 y egui-winit 0.33.3: commit nativo Alt+numpad huérfano, UTF-16 y cancelación, en la cola original y convertido en texto sin teclas falsas, IME sintético ni hook global. Ver alt-input-0.6.0.md. Verificación nativa pendiente del usuario. |
| F09 | Adaptador común para ScrollAreas, único dueño, cancelación por rueda/clic/Escape/foco y rectángulos de imagen pintada/enlaces por línea; el layout no se reduce para ocultar espacios. Botones/filas conservan su área funcional. |
| F10 | Fin normal del espectador/emisor distinto del error de acceso/cupo/decoder: se retiran recursos de visualización y controles caducos, conservando voz/cámara pertinentes. |
| F11 | Resolver puro de nombres con contexto explícito del origen antes del truncamiento; respeta literales/código/escapes, nickname explícito y fallback neutro. No consulta por toast ni toma nicknames de otro guild. |
| F12 | Retirado; sin cambios ni comprobación del problema de actualización de 0.4.1. La publicación de la nueva entrega sigue el flujo existente. |
| F13/F14 | Orden y altura pantalla→cámara; oferta de fuente separada de textura/decoder, y elección de cámara conservada en reemplazo/pausa de la misma llamada. Un anuncio retirado o una llamada nueva invalida las fuentes anteriores. |
| F15 | Catálogo con LRU de ocho entradas/cuatro MiB/60 s, cuatro trabajos y ocho observadores por trabajo, deduplicación/reutilización y correlación completa. Métricas numéricas de respuesta disponibles; no se hicieron benchmarks ni se declaran ahorros medidos. |
| F16 | Estrellas efectivas locales/remotas y un escritor de historial. Contrato remoto verificado en JS público de Discord: protobuf conserva bytes desconocidos, revisión requerida, intención durable y GET posterior incluso tras resultado incierto; sin repetir PATCH a ciegas. La lectura/escritura del historial es estricta y acotada; fallo local posterior al éxito remoto conserva conciliación pendiente. Ledger en gif-060-evidence.md. |
| F17 | Dos preferencias persistentes por cuenta en la navegación existente: privados/grupos visibles y servidores ocultos inicialmente. Guardado previo a confirmar y conservación de campos ajenos; no dependen del intento de llamada. |
| F18/F19 | Árbitro de ámbito/foco antes de shell y campos, restauración del editor principal compatible con cuenta/vista/canal, prioridad modal y portapapeles por capacidades. Texto mixto tiene prioridad explícita; búsquedas/filtros no crean adjuntos. |
| F20 | Enter de edición separa prefijo y texto posterior del mismo lote; la cola permanece como revisión nueva y su ACK anterior no la cierra. Formularios ambiguos conservan datos y requieren nueva confirmación; Shift+Enter, IME y campos multilínea mantienen su función. |
| F21/F22 | Secciones semánticas y helpers compartidos, target fijado al abrir y un propietario por contexto. Se identificó y corrigió una reentrada concreta: Response.clicked consultaba Context dentro de data_mut en record_message_text_gesture. Se precalcula fuera del guard. PopupState deja de crecer por cada fila y usa un único slot. Bloqueo explicado estáticamente; confirmación funcional pendiente. |

La revisión independiente encontró además y corrigió: cola después de Enter en el editor, ACK que cerraba una revisión posterior, navegación mayor al antiguo límite de lectura, historial ilegible que no debe reemplazarse con vacío, estado de popups sin cota y clases de error de colas que debían conservarse en diagnóstico. Se conservan las protecciones de generaciones, permisos, rate limits y corte de seguridad.

### Comprobaciones permitidas

- Revisión cruzada de T0/T5, voz/entrada nativa, menús/interacción y transformación/reconciliación de favoritos.
- Primera integración: cuatro errores de importación del comparador de snowflakes detectados por el compilador y corregidos antes de producción.
- `cargo check --workspace --all-targets --release --offline --locked -j 1`: pasó después de la integración en 25,15 s y tras el último ajuste de nombres en 19,11 s. Comprueba los destinos de tests sin ejecutarlos. Persisten advertencias de código sin uso y un literal f32 con aviso de compatibilidad futura; no impiden compilar esta entrega.
- Análisis de sintaxis de los cuatro scripts de distribución modificados: correcto; no ejecuta cliente/Setup.
- Preparación estática del runtime: 31 componentes, x64, hashes de referencia preservados; ningún motor ejecutado.
- Cargo-audit 0.22.2, DB RustSec commit `99670cfc0bc574433034b75876a4b791ccaa2efb` con 1380 avisos: cero vulnerabilidades reportadas, cuatro avisos informativos de mantenimiento (ttf-parser, zbus_names, zvariant, zvariant_derive). Se usó `--no-yanked`; no es auditoría completa del motor nativo ni prueba de los parches de entrada.
- Cargo conservó las versiones al introducir los dos paths de proveedor parcheados; reajustó seis referencias windows-sys entre versiones ya presentes en el lock. No se modificó la caché fuente global del proveedor.

Toda comprobación funcional de interacción, voz, GIFs/favoritos y entrada Windows queda para el usuario. No se ejecutaron cliente, instalador, tests sintéticos o funcionales, ni se utilizó una cuenta de Discord. No se tocó la instalación, accesos directos o datos de Plus. La compilación de producción, el instalador y publicación se registran después de generarlos y verificar sus identidades.
Último check tras reservar el presupuesto futuro de escritores de diagnóstico activos: pasó en 46,05 s. Se mantienen los límites aun si coinciden ejecuciones explícitas; fuentes congeladas para producción.
