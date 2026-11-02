# Plan para Claude: conexión, envíos, adjuntos e interfaz

Fecha: 31 de octubre de 2026, America/Lima. Estado: **implementado y compilado; pruebas y confirmación visual pendientes** (ver «Entrega de programación» al final). Preparado con `notdiscord-planificar` para que Claude programe esta tanda. Este documento contiene el alcance vigente y las correcciones posteriores del usuario; no requiere reconstruir la conversación anterior.

## Base y límites

- Repositorio observado: `E:/DESCARGAS/REA/NotDiscordRust`; aplicación: `NotDiscord-Rust/`. Rama **NotDiscord**, HEAD **`a67c8e66316dfefcdff481a6b2096cfb47ee4ac9`**, manifiesto estándar **0.9.0**. Los README aún muestran entregas antiguas; para la versión prevalece el manifiesto contrastado con Git.
- Al comenzar sólo existe `NotDiscord-Rust/evidence/` sin seguimiento; conservarlo. No se cambió código, versión, configuración ni datos del producto durante el análisis.
- Clasificación propuesta, inferida de correcciones funcionales y pequeñas adiciones: **Mediana, 0.9.0 → 0.10.0**, según `docs/plan-skills.md`. Es una propuesta: no se modifica el manifiesto por planificar. Si cambia la base antes de programar, revalidar fuentes y versión, sin renumerar una entrega publicada.
- **Todo lo referente al error de llamadas queda retirado**, por instrucción posterior del usuario: confirmó que era un fallo del servidor de Discord. No investigar ni corregir voz, audio, cámara o transmisión como parte de esta tanda.
- Se mantienen NotDiscord+ y legacy fuera del alcance. F12 del plan anterior, relativo al actualizador, sigue retirado; el control de actualización sólo es referencia visual para el aviso de conexión.
- Este pedido autoriza preparar el plan para Claude, no implementar, compilar, ejecutar cliente o instalador, publicar, utilizar cuentas ni realizar pruebas funcionales. En la implementación futura siguen vigentes las restricciones del proyecto: revisión y compilaciones permitidas según el pedido; no ejecutar cuentas, dispositivos, llamadas ni pruebas funcionales/sintéticas para sortearlas. Los criterios de aceptación de este documento **no son resultados de pruebas**.
- «Ventana externa» se interpreta como un panel flotante de la aplicación, como ajustes/visor/diagnóstico de las capturas. Reutilizar la infraestructura egui; no hace falta crear otro proceso, WebView o ventana de Windows para estas funciones.

El plan anterior de interacción documenta 0.6/0.7 y su cierre. Esta es una tanda nueva sobre 0.9.0, con contrato de entrega independiente para Claude; no reabre automáticamente todos los requisitos anteriores.

En las evidencias, `app/src/` y `core/src/` abrevian `crates/app/src/` y `crates/core/src/`; `ui/` pertenece a `crates/app/src/ui/`, y `controller/` a `crates/app/src/controller/`. `controller.rs` sin prefijo es el de app. Todas las rutas parten de `NotDiscord-Rust/`.

## Alcance solicitado

| ID | Prioridad funcional | Resultado solicitado |
|---|---|---|
| H01 | P1 | Aviso bajo la sección de usuario al perder conexión general, con estado y opción de reintentar |
| H02 | P2 | Guardar y quitar GIFs favoritos desde el propio mensaje del chat |
| H03 | P2 | Leer adjuntos de texto desde el mensaje y desde el compositor mediante un visor flotante |
| H04 | P2 | Tooltip de reacciones con los displaynames de quienes reaccionaron |
| H05 | P1 | Mensaje local inmediato con presentación normal; fallo rojo y enlace rojo de reintento |
| H06 | Retirado | Error de llamada de las capturas 5/6: excluido expresamente por el usuario |
| H07 | P2 | Diagnóstico con cierre exterior coherente, copiar todo y abrir carpeta seleccionando el archivo pertinente |
| H08 | P2 | Muestra de color junto al HEX y editor de color hijo con previsualización en ajustes |

P1/P2 ordenan el trabajo funcional; no declaran vulnerabilidades ni una severidad de seguridad. **C0, la política conservadora de red, es un requisito transversal de aceptación**, especialmente en H01/H05.

## C0 Política conservadora de red solicitada posteriormente

El usuario pide minimizar posibles riesgos para su cuenta: ante fallos del servicio, **una operación en curso, esperar su respuesta/finalización y no generar ráfagas**. La solución consiste en limitar tráfico y respetar los mecanismos existentes; no introducir ocultación, rotación de identidad, evasión de verificaciones ni afirmar que el cliente garantiza evitar sanciones.

1. Un único dueño de recuperación por cuenta/generación y, como máximo, un intento de conexión activo. Coalescer clics repetidos. Una conexión persistente existente no cuenta como un intento nuevo cada vez que transmite un frame obligatorio.
2. Al perder el transporte, permitir como máximo **un intento automático de recuperación**. Si falla, **detener la serie** y esperar una acción explícita. No conservar los seis intentos secuenciales actuales ni sustituirlos por bucles en UI, controlador, timers o HTTP. La apertura inicial tampoco debe encadenar seis intentos tras fallar.
3. Un intento comprende los pasos obligatorios del protocolo: conexión, Hello, Resume o Identify pertinente y confirmación Ready/Resumed; conservar heartbeats, orden, límites e intervalos de Identify. No significa enviar un único frame y omitir el protocolo. No encadenar una sesión nueva adicional después de un rechazo/timeout dentro del mismo presupuesto sin representarlo y autorizarlo en la política.
4. No habilitar otro intento hasta que el anterior haya concluido o su tarea/transporte haya sido cerrado y esperado. Timeout local no demuestra cancelación del trabajo ni rechazo del servidor. Un POST que pudo salir mantiene resultado desconocido aunque se cancele su tarea local.
5. Nuevo intento manual sólo tras cooldown. Propuesta inicial para un intento de recuperación fallido: mínimo **30 segundos**, ampliado por cualquier plazo mayor ya impuesto por Discord, rate limit o política del protocolo. Mostrar espera y deshabilitar acción mientras corresponda. Este valor es de diseño, no un límite oficial ni una promesa sobre sanciones.
6. Enviar/reintentar mensajes sólo por intención explícita; **ningún reenvío automático al reconectar**, ninguna descarga masiva de la cola local. Serializar los reintentos de recuperación de mensajes por cuenta; no crear varios POST simultáneos por doble clic o por varios mensajes fallidos.
7. Suspender durante la desconexión los productores opcionales del transporte general: typing, ACK de lectura, demandas de catálogo/reacciones y sincronizaciones periódicas. Descartar typing obsoleto y coalescer el estado que deba conservarse con límites. Al recuperar, reactivar demanda visible de forma gradual; no reproducir todos los eventos acumulados.
8. Respetar `Retry-After`, rate limits globales/de ruta y los cortes `Authentication/Verification/Protocol`. No reintentar automáticamente un POST, PATCH o DELETE con resultado ambiguo. Para las nuevas consultas de recuperación usar una política explícita sin reintento automático oculto: `Api::request` tiene hoy hasta tres intentos de lectura en determinados 429 (`core/src/api.rs:352–386`). El usuario no debe provocar una serie sin saberlo.
9. Consultas y mutaciones que necesiten varios pasos, como leer favoritos → modificar → confirmar, esperan cada respuesta antes del siguiente paso. Un fallo detiene la secuencia; una escritura incierta no se repite. No se exige reducir una transacción necesaria a una petición imposible, sino evitar solapamientos y repeticiones.
10. La conciliación de un envío incierto usa primero la evidencia ya recibida. Si hace falta consulta, será explícita, acotada y sin polling ni recorrido de historial masivo. No encontrar un mensaje en una página parcial no demuestra que no se envió.
11. Conservar `Operations.Unknown`, límites de tráfico y asociaciones de envíos entre recuperaciones del transporte. No reconstruir `Api` para borrar esas barreras ni convertir «Recuperar borrador» en un nonce nuevo que las eluda.

La coordinación es **compartida por cuenta**, no dos semáforos independientes para H01 y H05. Mientras exista recuperación activa, cooldown de desconexión o conexión sin sincronizar, H05 y las demandas opcionales no inician tráfico, aunque `SafetyGate` siga `Allowed`. Se puede conservar la intención local, pero recuperarse no la envía automáticamente: necesita otra acción explícita válida. Los frames obligatorios del protocolo conservan su excepción. Durante ese intervalo, la aceptación debe observar cero POST/GET opcionales nuevos, no sólo un máximo de una conexión.

Los registros de recuperación sólo usan clases de resultado, fases, contadores y correlaciones efímeras; nunca credenciales, texto de mensajes, portapapeles, IDs privados ni URLs firmadas.

## H01 Desconexión general y fila bajo el usuario

**Estado actual confirmado.** `core/src/gateway_runtime.rs:31–41,595–818` ya tiene timeouts, heartbeat, reanudación, cooldown, límites de Identify y seis intentos de recuperación. No faltan todos los handlers. `app/src/controller.rs:250` copia `GatewayEvent::Status(String)` al snapshot; `core/src/models.rs:504–515` no representa con tipos el intento, causa o acción de recuperación. Cuando termina inesperadamente `gateway_rx`, el controlador sólo retira el receptor y puede conservar un «En línea» obsoleto.

El banner superior de seguridad (`ui/shell.rs:708–736`) pertenece a otra política; su «Volver a conectar» para sincronización envía `login`. `connect_secret`, `controller.rs:878–893`, restablece la vista de sesión; `ui/mod.rs:896–930` puede limpiar borradores, capturas y temporales al observar ese cambio. **No usar esa ruta como reconexión transparente**.

**Cambio.** Añadir estado tipado de conexión/sincronización general, independiente de presencia y del resultado de una petición aislada: conectado y sincronizado, desconectado, recuperando, esperando acción/cooldown y bloqueo con acción específica. El aviso no se activa por `invisible/offline` elegido por el usuario ni por un 403 de permisos o un 429 aislado. Un error de red de una operación puede mostrar su fallo sin declarar caída global del servicio.

El propietario de sesión coordina el comando de recuperación bajo C0. Reutilizar credencial protegida y sesión de transporte cuando proceda, sin abrir el login como efecto de cada clic. Conservar borradores, selección, cachés válidas, pendientes, `Operations` y `RateLimits`. Un bloqueo terminal mantiene su acción propia; no hacer que el botón genérico desbloquee un `SafetyGate` monotónico (`core/src/safety.rs:99–107`). Distinguir cierre inesperado de logout, cierre de aplicación y cancelación intencional.

Añadir fila reutilizable **inmediatamente debajo del panel de usuario**, con estilo coherente con actualizar: aviso breve, «Reintentar conexión», estado de intento y espera. La actualización conserva su control independiente. Reservar altura en ambas ramas de navegación: `left_footer_height`, `ui/shell.rs:223–229`, y callers `783,935`; dibujar junto al footer de `1982–2125`. No marcar conectado sólo por abrir el socket; esperar sincronización necesaria. La recuperación no reenvía mensajes.

**Aceptación pendiente:** pérdida/fin inesperado no deja «En línea»; un intento fallido queda detenido; diez clics rápidos no producen diez conexiones; cooldown no se reinicia al repintar ni al cambiar de canal. Ready/Resumed retira el aviso; borradores/adjuntos/pendientes sobreviven. Presencia invisible no lo activa. Cortes de seguridad conservan su acción específica. Aviso y actualizar caben con ventana pequeña/DPI 100/150 %, sin ocultar usuario ni cubrir navegación.

## H02 Favoritos de GIF desde el chat

**Brecha confirmada.** `ui/chat.rs:1774–1799`, `media_menu`, y `ui/chat/context_menu.rs:162–169` ofrecen abrir, guardar original, copiar y retry, pero no favorito. `Target::Media` sólo lleva URL/poster/nombre. El catálogo ya combina descriptores locales/remotos (`ui/dialogs.rs:1846–1881`), pero los remotos se solicitan al abrirlo: una lista todavía no cargada **no significa que el GIF no esté guardado**.

**Cambio.** Compartir entre chat y catálogo un resolvedor de estado y una acción de intención explícita `saved=true/false`, con descriptor multimedia, tipo acreditado, cuenta y generación. Extender el target, sin detectar animación sólo por extensión: las páginas/proxies/renditions pueden no terminar en `.gif`. Mostrar estrella/acción «Guardar GIF» o «Quitar de favoritos» en el menú contextual del medio y una estrella accesible al pasar sobre el GIF, sin tapar Play/Ampliar. La imagen 1 no exige empezar a reproducir ni abrir el catálogo para guardarlo.

Representar estado remoto desconocido/cargando/listo/error; deduplicar una demanda al abrir el menú o consultar la estrella. No hacer HTTP en cada frame ni por cada GIF visible. Respetar C0 al estar desconectado. La fuente canónica y aliases página/poster/rendition comparten identidad y evitan duplicados.

Reutilizar **el escritor único** de `controller/gifs.rs:286–405` y las acciones del catálogo (`ui/dialogs.rs:2053–2066`). No crear otro escritor ni usar el toggle local legado `controller/social.rs:685–700`. Mantener la política vigente: alta local en NotDiscord; si ya está guardado en Discord, quitarlo también allí y de la copia local, según la instrucción previa del usuario. No ampliar silenciosamente el alta a una nueva escritura remota. La baja remota conserva lectura actual, `required_data_version`, journal y confirmación canónica; no repetir PATCH ambiguo.

Adaptar a C0 la rama actual de lectura posterior a error (`controller/gifs.rs:371–379`): después de un PATCH confirmado puede realizarse la lectura canónica secuencial del intento; ante error/timeout incierto se detiene antes de una nueva consulta automática, conserva el journal y ofrece **«Comprobar estado»** explícito para una única lectura permitida. Esa lectura no vuelve a escribir ni demuestra por sí sola el resultado si también falla. Así se conserva la conciliación sin mantener una excepción oculta a la preferencia de red del usuario.

**Aceptación pendiente:** guardar desde chat aparece en Guardados; chat y catálogo muestran la misma estrella; desmarcar uno remoto lo retira también de Discord cuando se confirme. Estado remoto desconocido no permite una acción falsa de alta. Doble clic, offline, error de persistencia, cambio de cuenta o respuesta antigua no duplican ni anuncian éxito falso. Conservar límites actuales de 100 locales/400 remotos y los presupuestos de descargas/cachés.

## H03 Visor de adjuntos de texto

**Brecha confirmada.** El adjunto no imagen del mensaje usa `platform::open_link` (`ui/chat.rs:807–813`). Los chips del compositor sólo tienen preview para imágenes (`2065–2089`, `previewable_path:3479–3484`). No hay visor TXT. `ui/txt_import.rs` aporta un patrón de lectura acotada fuera de UI, pero su parser de enlaces/favoritos **no es un lector de documentos**.

**Cambio.** Un mismo visor flotante de sólo lectura para dos fuentes tipadas: adjunto local preparado del compositor y adjunto remoto acreditado de un mensaje. Al pulsar el nombre/chip de un `.txt`, abrirlo dentro de la aplicación, sin enviar el mensaje, lanzar navegador/editor ni ejecutar contenido. Alcance inicial: `.txt` y adjuntos declarados `text/plain`; otros tipos conservan su acción actual. MIME/extensión seleccionan el recorrido, no demuestran que los bytes sean texto válido.

Usar shell/cierre/foco compartidos con H07/H08, título con nombre, estados cargando/error, texto plano monoespaciado seleccionable y copia explícita. Scroll vertical y horizontal o ajuste de línea controlado; virtualizar para que una línea gigante o muchas líneas no bloqueen la UI. Mostrar HTML, scripts, Markdown y enlaces **como texto**, sin WebView, evaluación ni navegación automática.

Presupuestos **propuestos**: entrada de hasta **1 MiB**, resultado UTF-8 de hasta **2 MiB**, una carga activa y un resultado; lectura de límite+1 para detectar exceso. Admitir UTF-8 con/sin BOM y UTF-16 LE/BE con BOM, con validación estricta y límite de salida; no adivinar codificaciones binarias ni truncar silenciosamente. Archivo vacío es válido. Estos límites son de preview, no cambian los de envío ni justifican aumentar el límite de imágenes.

Local: leer sólo el archivo ordinario del adjunto preparado, validar el handle y limitar bytes aunque cambie su tamaño; no interpretar texto recibido como ruta local. Mantener referencias mientras esté abierto y no borrar el adjunto al cerrar el lector.

Remoto: conservar cuenta/generación, canal/mensaje/adjunto y apertura; verificar permiso de lectura y correspondencia antes de presentar el resultado. Descargar fuera de UI, con cliente sin token/cookies, HTTPS y hosts/CDN permitidos, límite de bytes durante streaming, timeout y validación de redirecciones. El cliente multimedia sin redirección automática de `ui/resources.rs:370–379` es referencia de transporte, no un decodificador TXT. URL vencida: ofrecer recuperación acotada de metadata del **mismo** adjunto, sólo tras acción explícita y bajo C0; no inventar firmas ni sondear endpoints. Cerrar, reemplazar fuente, revocar lectura o cambiar cuenta cancela y descarta resultados antiguos.

No registrar ni persistir texto o URLs firmadas, ni mantener caché ilimitada. Si se necesitan temporales, serán propios, con limpieza por último propietario.

**Aceptación pendiente:** el `.txt` del mensaje y del compositor usan el mismo lector; copiar no envía nada. Cubrir Unicode, BOM, UTF-16, vacío, límite exacto/exceso, línea enorme, binario, archivo local cambiante, error de red, URL vencida y resultado tardío. Cerrar conserva borrador, adjuntos y posición del chat. Permisos retirados impiden seguir mostrando contenido.

## H04 Tooltip de reacciones con displaynames

**Ya existe un recorrido que hay que completar/revisar.** `ui/chat.rs:821–850` llama al tooltip compartido por chat, pins y búsqueda; `ui/chat/reaction_tooltip.rs:168–330` ya muestra nombres/avatares, carga, error y reintento, con reacciones normales/superreacciones. Tiene dwell de **1,2 s**, reiniciado por movimiento mayor a 3 px (`219–228`), caché de 32 entradas/TTL 30 s y timeout 10 s. `controller/reaction_users.rs:61–107` limita una consulta y una pendiente reciente. No se reprodujo una ausencia de función: que el dwell o contexto expliquen la captura es una hipótesis.

**Cambio.** Conservar panel y consultas existentes. Ajuste de UX propuesto: tooltip al mantener el puntero aproximadamente **500 ms**, con corredor hacia el panel para poder leer/copiar/desplazarse. Proporcionar carga y error legibles; no presentar un error como «nadie reaccionó». No crear otro fetcher por botón, perfil o frame. Reutilizar caché, coalescer y detener reintentos automáticos según C0.

Corregir también la expiración de errores: hoy el TTL puede retirar el error (`reaction_tooltip.rs:79–82`) y el siguiente frame del mismo hover iniciar otro GET (`235–249`). **Expirar datos no rehabilita automáticamente un intento fallido.** Conservar un marcador acotado de error/detención; reintentar requiere el botón explícito y la coordinación C0. Ni permanecer encima, mover el puntero ni repintar deben convertir el TTL en un bucle de peticiones durante una caída.

Displayname: nickname del servidor **del mensaje** cuando esté disponible, después `global_name` y finalmente username. `core/parsing.rs:46–55` ya resuelve nombre global; el tooltip (`301–302`) aplica nickname sólo para canal seleccionado. Completar el contexto de origen en pins/búsqueda sin usar apodos de otro servidor ni consultar perfiles en cada hover. No mostrar IDs como sustituto del nombre.

Mantener límites de usuarios/paginación del panel, indicando lista parcial si aplica; no cargar todos los miembros para un tooltip. Invalidar también cuando cambien los usuarios de una reacción sin variar el conteo, mediante eventos/revisión/TTL acotado. Respetar permisos, borrados, navegación, generación y cuenta.

**Aceptación pendiente:** hover muestra displaynames en chat/pins/búsqueda y reacciones normales/superreacciones. Poder mover el puntero al panel sin cerrarlo de inmediato. No confundir carga/error/vacío. Cambios con igual conteo no quedan indefinidamente obsoletos. Varias entradas rápidas no disparan varias peticiones simultáneas ni consultas tras perder acceso.

## H05 Mensaje inmediato, fallo rojo y reintento conservador

**Interpretación confirmada al usuario:** tras una intención local válida de enviar, el mensaje aparece inmediatamente en el historial con el aspecto completo de un mensaje normal, sin esperar respuesta de Discord ni mostrar la tarjeta «Enviando…». Si falla, el texto queda rojo y debajo aparece **«Click aquí para reintentar enviar el mensaje»**, también rojo, subrayado al pasar el mouse y accionable. Esto cambia la presentación; no representa una confirmación remota ficticia.

### Lo existente y las brechas

- `ui/chat.rs:2418–2440` ya valida/captura Draft, conserva archivos y vacía el editor inmediatamente. `2469–2494` conserva identidad al reintentar. Reutilizarlo; no crear una segunda outbox.
- `draw_pending_messages`, `2496–2532`, dibuja hoy tarjeta aparte, «Enviando…» o «Envío sin confirmar», texto recortado a 300 caracteres y contador de archivos. Ese es el recorrido que debe reemplazarse.
- `controller.rs:1744–1793,2495–2501` genera nonce estable y usa `enforce_nonce`. HTTP confirma por `__sent` (`2207–2223`, `ui/mod.rs:1084–1116`).
- `core/src/models.rs:319–362`, `parsing.rs:930+` no conservan nonce; `controller/incoming.rs:236–356` deduplica `MESSAGE_CREATE` sólo por ID remoto. No confirma la captura local por nonce. Gateway antes de HTTP puede mostrar dos filas; Gateway correcto seguido de timeout HTTP puede dejar un falso fallo.
- El controlador convierte el error API a `String` (`controller.rs:1793`); `send-error` no conserva clase ni `retry_after` (`467–469,663–665`).
- `core/src/api_operations.rs:78–89,122–138` mantiene resultado `Unknown` y bloquea repetir la misma mutación. Es una protección, no un handler que deba eliminarse para que funcione el botón.
- «Recuperar borrador» (`chat.rs:2515–2522`) elimina submission; el siguiente envío crea identidad nueva. Si el anterior era desconocido, esto puede saltarse la barrera y producir un duplicado remoto.

### Contrato de implementación

Modelo local: cuenta/generación/canal, submission/nonce estable, revisión capturada, autor/hora, contenido completo, menciones/respuesta, archivos y estado tipado. Distinguir preparado, pendiente, confirmado, fallo concluyente/reintentable y resultado desconocido. El borrador nuevo es independiente del ya capturado. Las prevalidaciones que impiden una intención válida conservan el borrador; no enviar ni limpiar antes de validar permisos y límites.

Compartir layout/Markdown, selección, geometría de adjuntos y orden del historial con los mensajes remotos, mediante una proyección local. No insertar IDs inventados en la caché canónica ni habilitar pin, borrado remoto, reacción o reenvío remoto de un mensaje todavía sin ID real. Conservar identidad visual y ancla al confirmar; el cambio local → canónico no debe saltar ni reordenar otras capturas. El mensaje propio local no genera notificación ni contador de no leídos/mención.

Un único propietario concilia respuesta HTTP y `MESSAGE_CREATE` por nonce normalizado, autor/cuenta y canal. Preservar nonce string/integer sin pérdida de precisión. Confirmar una sola vez y sustituir la proyección por el mensaje canónico. Confirmado no vuelve a fallido por un HTTP tardío; replay de Gateway no duplica. Si no hay nonce, un mensaje de igual texto/hora **no basta** para afirmar identidad.

La [documentación oficial de mensajes](https://github.com/discord/discord-api-docs/blob/main/developers/resources/message.mdx) confirma el nonce en Message Create y limita la deduplicación de `enforce_nonce` a una ventana de pocos minutos. No asumir idempotencia indefinida ni inventar su duración exacta. Esa limitación obliga a conservar las barreras locales al reintentar.

| Resultado interno | Presentación y acción |
|---|---|
| Pendiente | Mensaje normal completo, sin «Enviando…» ni afirmación de entrega; no ofrecer otro POST |
| Confirmado por HTTP o Gateway conciliado | Una sola fila canónica, sin error ni acción de retry |
| Fallo concluyente recuperable/no llegó a enviarse | Texto rojo y enlace solicitado; una pulsación inicia un único intento tras validar permisos, estado y cooldown |
| 429 o espera impuesta | Error rojo con explicación breve de espera; enlace deshabilitado hasta el plazo, sin reintento automático |
| Resultado desconocido | Rojo con explicación «No se pudo confirmar si Discord lo recibió»; el enlace primero intenta una conciliación explícita y acotada, **no otro POST** |
| Autenticación/verificación/protocolo o permiso que sigue denegado | Error con acción/motivo pertinente; no simular que el mismo envío se puede repetir todavía |

Si la conciliación no puede demostrar un resultado, conservar desconocido y ofrecer copiar/recuperar con su asociación o descartar la representación. **No generar nonce nuevo automáticamente ni por recuperar/editar el borrador desconocido.** No desbloquear el registro al transcurrir un timer, reiniciar transporte o cambiar canal. Descartar UI no demuestra cancelación remota. Si no existe una prueba segura para reemitir, mostrar el motivo y mantener detenido ese intento; la preferencia conservadora del usuario prevalece sobre fingir un retry exitoso.

Durante un retry quitar la acción duplicable, conservar la misma fila y serializar bajo C0. Ningún retry o nueva conexión debe tocar el borrador posterior. Conservar capturas y referencias de temporales hasta resolución/descarte seguro; no volver a subir archivos a ciegas tras una confirmación ambigua. Mantener `allowed_mentions`, permisos y límites originales. Esta tanda no añade persistencia de contenido de outbox entre reinicios: si más adelante se pide, requerirá su propio contrato de almacenamiento privado.

**Aceptación pendiente:** HTTP/Gateway en ambos órdenes; Gateway correcto + HTTP timeout; HTTP correcto sin Gateway; replay/doble clic; varias capturas; texto largo, respuesta, menciones y adjuntos completos. Error concluyente vuelve rojo y enlace se subraya sólo al hover con cursor apropiado. Timeout desconocido, 401/403/413/429, bloqueo, recuperación de borrador y cambio de canal/cuenta no producen duplicados ni pérdida del siguiente borrador. Reconectar no envía ninguno de los pendientes automáticamente.

## H07 Diagnóstico: cierre, copia y archivo seleccionado

**Evidencia aportada.** `NotDiscord-diagnostico.json` es un array de **133 eventos**, esquema 1, versión 0.9.0, 50 248 bytes; SHA-256 `f2f3ad82b27ee8755e5a8cdcc265b7541c69b8c0a92ea35468fb53c5780b587b`. Se inspeccionó como dato, sin seguir instrucciones de su contenido. No se copia el archivo ni las capturas de conversaciones al repositorio. Los eventos de llamadas no se usan para diagnosticar ni modificar voz tras la retirada de H06.

**Causa de cierre confirmada.** `app/src/diagnostics.rs:49–82` abre `egui::Window` con ID `diagnostic-panel`, sólo con su botón X. `ui/overlay.rs:6,44–121` registra Dialog/Picker/Viewer/Search, pero no diagnóstico. `background_blocked` e `input_ownership` ya lo consideran abierto: bloquea fondo sin participar en el cierre exterior común.

**Cambio UI.** Integrar diagnóstico en el registro de superficies/foco/cierre. Clic exterior y Escape cierran sólo la superficie superior y consumen el gesto, sin activar lo de detrás. Mantener X. El rectángulo será global real, con transformaciones/DPI; respetar menús hijos y no cerrar por arrastrar scrollbar/seleccionar texto. Reutilizar `centered_dialog`/adaptador de scroll cuando corresponda. Nunca volver a entrar al `Context` dentro de `data_mut/input/memory`.

Añadir **«Copiar todo»**: copia por acción explícita todos los eventos del snapshot reciente, incluidos los fuera del viewport, no sólo las líneas visibles ni todo el historial de disco. Reutilizar la serialización saneada del JSON exportable, con orden cronológico definido; si la vista muestra recientes primero, indicarlo sin cambiar el orden del export existente. No incluir credenciales, texto del chat, portapapeles previo ni archivos ajenos. Feedback breve de copia/error; preparar una vez por acción, sin serializar todo de nuevo cada frame.

**Snapshot coherente.** `core/src/diagnostics.rs:199–207` devuelve `[]` tanto si el ring está vacío como si `try_lock` falla/no hay recorder; `export()` puede guardar ese vacío como éxito. Introducir resultado fallible que distinga vacío real, ocupado y no disponible, sin bloquear UI. Conservar último snapshot válido marcado como tal. Panel/copia/export parten del mismo snapshot capturado para la acción; eventos nuevos no cambian su contenido a mitad. Mantener ring de 256 y límites actuales, no ampliar a lectura de archivos arbitrarios.

**Abrir carpeta seleccionando archivo.** Actualmente `app/src/diagnostics.rs:59–62` sólo pasa el directorio a Explorer. El recorder conoce el `.jsonl` de ejecución al crearlo (`core/src/diagnostics.rs:139`), pero no expone su ruta. Conservarla explícitamente. Política propuesta para evitar ambigüedad: sin exportación, mostrar y seleccionar el registro de esta ejecución; después de exportar correctamente, mostrar «Archivo exportado: …» y seleccionar ese JSON como archivo pertinente. Conservar ambas identidades y permitir volver al registro de sesión; nunca elegir por «archivo más reciente» de la carpeta.

Usar helper de plataforma para seleccionar el archivo, con rutas Unicode/espacios y error si falta. La [API de Shell `SHOpenFolderAndSelectItems`](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shopenfolderandselectitems) abre su carpeta con el elemento seleccionado y requiere inicialización COM. Ya están habilitados Shell/COM en `crates/app/Cargo.toml:42`; reutilizar ese soporte, liberar PIDLs/COM correctamente y no construir un comando de shell concatenando la ruta. El log en disco puede contener más eventos que el ring: explicar brevemente que el panel es el resumen reciente.

**Exportación robusta.** La implementación actual usa `create_new` y escribe directamente: un error puede dejar JSON parcial y el siguiente intento con el mismo nombre falla. Capturar snapshot antes, escribir temporal propio y publicar el archivo final sólo tras éxito/flush, conservando rechazo de enlaces y política de no sobrescribir. Guardar la ruta de export sólo al confirmar éxito. No esperar disco lento dentro de pintura; operación acotada, con error/cancelación entregados a UI. Limpiar sólo el temporal propio ante fallo, sin borrados recursivos.

**Aceptación pendiente:** exterior/Escape/X como otros paneles; no click-through ni cierre por interacción interna. Copia y export de un snapshot contienen todos sus eventos y el mismo orden, incluso con contención o eventos nuevos. Contención no se presenta como «sin eventos». Export cancelado/fallido no actualiza archivo seleccionado ni anuncia éxito; no deja final parcial válido aparente. Explorer selecciona el archivo mostrado, incluido nombre Unicode/espacios. Archivo ausente/disco no disponible muestra motivo y no abre otro fichero arbitrario.

## H08 Editor de colores hijo de ajustes

**Estado confirmado.** `ui/appearance.rs:77–83` sólo tiene TextEdit para `background`, `separator` y `accent_color`. `theme::parse_color`, `ui/theme.rs:258+`, admite `#RRGGBB`. Ajustes ya previsualiza y tiene backup, Guardar/Cancelar (`appearance.rs:120–141`; cierre en `dialogs.rs:1410+` y `overlay.rs:93+`). La ventana conserva transformación de escala propia mientras está abierta (`appearance.rs:32–48`).

**Cambio.** Fila reutilizable por campo: etiqueta, **botón con muestra del color actual** y HEX editable a su derecha. Tooltip exacto del botón: **«Cambiar color»**. Al pulsarlo, abrir editor flotante **encima de ajustes**, manteniendo padre visible y su borrador. Identificar campo por enum/tipo (Base/Separadores/Acento), no por cadenas arbitrarias.

Reutilizar el color picker de egui fijado por `Cargo.lock` en 0.33.3, con modo opaco, dentro de la ventana hija y controles HEX/RGB. La fuente local `egui-0.33.3/src/widgets/color_picker.rs:467–489` ya tiene `color_picker_hsva_2d`/`color_picker_color32`; no se necesita dependencia ni selector nativo de otro proceso. Paleta bidimensional y barra de tono/valor según widget, RGB 0–255 y HEX; no se exige copiar píxel a píxel la captura de Windows ni añadir cuentagotas. Opacidad sigue siendo el ajuste independiente existente.

Cada cambio válido del editor se refleja **en vivo** en la muestra, HEX de ajustes y tema provisional, con una sola fuente de color y sin escritura a disco por arrastre. Mantener el buffer de HEX incompleto/incorrecto durante edición, mostrar error sin panic ni reemplazarlo cada frame por otro valor. Guardar no acepta código inválido. Evitar bucles de conversión RGB/HSV/HEX que cambien valores sin acción del usuario; conservar estado de tono en colores sin saturación.

Contrato de anidación: el editor de color es estado hijo, **no reemplaza `Dialog::Settings`** ni consume su backup. «Aplicar» acepta el color en el borrador del padre; «Cancelar», Escape, X o exterior del hijo restauran sólo el color que tenía al abrirlo. El primer clic fuera cierra el hijo y deja ajustes; no activa controles del padre ni lo cancela también. Guardar del padre persiste; Cancelar/cierre exterior del padre restaura su backup completo, incluidos cambios ya aplicados en el hijo. Reset/preset/cierre de cuenta invalidan o actualizan el hijo de forma explícita, sin devolver resultados a otra apertura de ajustes.

Integrar capa superior, rectángulo global y prioridad de teclado en el registro común de H07 y en `input_ownership`; los atajos del hijo no se envían al chat ni a Guardar del padre. No añadir simplemente otro `Some(Dialog::...)` que cierre ajustes. Considerar la transformación del padre al cambiar escala y abrir/cerrar a DPI alto.

Integrar asimismo el cierre ordenado existente: `ui/overlay.rs:20–24`, `prepare_orderly_close`, guarda `prefs` y consume backup, y se invoca también al reemplazar una instancia (`main.rs:29`, `ui/mod.rs:1536`). Antes de esa escritura debe cancelar/resolver el preview hijo no aceptado, validar HEX y aplicar la política existente del borrador padre. No persistir un color provisional por ese bypass ni consumir el backup si falla la validación/guardado. Esto adapta el ciclo de vida del editor al helper existente; no reabre F12 ni modifica el flujo de actualización.

**Aceptación pendiente:** los tres colores tienen muestra y HEX sincronizados; cambiar RGB/paleta/HEX refleja preview inmediato en el padre sin persistencia prematura. Aplicar hijo + Cancelar padre restaura todo; Cancelar hijo conserva otros cambios del padre. Cierre ordenado con hijo abierto no persiste preview no aceptado y un fallo conserva la recuperación del padre. HEX parcial/inválido, negro/blanco/gris, presets, reset, Enter/Escape/Tab, resize y DPI 100/150 % no pierden foco, valor, backup ni provocan clic al fondo.

## Orden de trabajo y propiedad de archivos

| Tanda | Dependencias y alcance | Entregable revisable |
|---|---|---|
| T0 Contratos | Revalidar HEAD/versión y restricciones; C0; snapshots y tipos de resultado/identidad | Estados, presupuestos y cancelación definidos antes de cambiar botones |
| T1 Recuperación | T0; H01 + política Gateway/API y productores opcionales | Un intento, espera/cooldown y fila bajo usuario sin reset de borradores/barreras |
| T2 Envío | T0/T1; H05, nonce HTTP/Gateway y outbox existente | Una fila normal inmediata; fallos rojos; retry conciliado, explícito y sin duplicados |
| T3 Superficies | T0; registro de superficies/foco/cierre + H07 | Diagnóstico coherente, snapshot/copia/export/selección de archivo |
| T4 Colores y texto | T3; H08 y H03; aplicar C0 al texto remoto | Editor hijo con transacción del padre; lector acotado en ambos orígenes |
| T5 GIF y reacciones | T0/T1; H02/H04; puede avanzar en paralelo con T3/T4 si tiene propiedad de archivos | Favoritos compartidos y tooltip con nombres de origen correcto |
| T6 Integración | Todas; revisión independiente, checks permitidos y estado por requisito | Evidencia de verificación y límites, sin marcar comportamiento probado sólo por compilar |

Asignar un integrador para `ui/chat.rs`, `ui/mod.rs`, `dialogs.rs` y `controller.rs`; varios requisitos los tocan. Delegar módulos nuevos/helpers o rangos con dueño explícito, sin parches simultáneos incompatibles. La tarea de superficies debe ser pequeña y reutilizable para diagnóstico, lector y color, no una reescritura general de todas las ventanas.

Al implementar, conservar mensajes serializados anteriores mediante campos opcionales/defaults (incluido nonce), límites de cachés/outbox y formatos existentes. Las preferencias de color siguen `#RRGGBB`, sin migración de opacidad. No migrar cuentas ni introducir persistencia privada adicional. Si se abandona un intento, conservar datos recuperables y asociaciones de resultado desconocido; rollback visual no revierte una mutación aceptada en Discord.

## Comprobaciones y entrega a Claude

Trabajo realizado: inspección estática de fuentes y diagnóstico aportado, contraste de capturas, lectura de contratos públicos de nonce/Shell y revisión separada de conexión/envío, medios/reacciones y diagnóstico. Se distinguieron las funciones ya implementadas de las brechas y se retiró H06. **No se ejecutaron el producto, pruebas, cuentas, portapapeles, dispositivos ni llamadas, ni se consultaron endpoints autenticados.**

Durante esta planificación sólo se validan documento, referencias, diff y estabilidad de base. Para programación, seguir las instrucciones vigentes del checkout y del pedido: revisión estática y compilación de producción/destinos pertinentes si está autorizada; los casos de aceptación sirven para diseñar regresiones, no autorizan ejecutar fixtures/pruebas funcionales o abrir una cuenta. Registrar claramente lo compilado frente a lo confirmado por el usuario. No hace falta instalar ni publicar para implementar el código, salvo que el pedido posterior lo requiera.

Cierre documental: `git diff --no-index --check` del plan sin errores de espacios; ocho huellas de fuentes coinciden y HEAD sigue en la revisión indicada. Sólo se añade este documento; `evidence/` permanece sin alterar. Las revisiones independientes de conexión/envío, medios/reacciones y diagnóstico/colores quedaron incorporadas.

Incluir en la entrega de programación: estado H01–H08 (H06 retirado), cumplimiento C0, causas ajustadas si aparecen otras, cambios y comprobaciones reales, versión resultante conforme a la política y pendientes de confirmación visual/servicio. No conservar en el producto las etiquetas internas H/T ni detalles de implementación innecesarios para el usuario.

Casos transversales de revisión: doble clic y teclas repetidas; timeout con tarea todavía activa; cancelación antes/después de escribir a red; resultado tardío de cuenta/apertura anterior; servicio sin respuesta, 429/cooldown y corte de seguridad; cambio de canal; borrador posterior; foco/scroll/clipboard entre padre e hijo; presupuestos de RAM/trabajo UI y fallos de disco. No afirmar ahorros medidos ni que se evita un baneo por estas comprobaciones.

### Estado de continuación

- [x] Revalidar base 0.9.0 y recoger siete requisitos activos.
- [x] Retirar el incidente de llamadas por instrucción del usuario.
- [x] Incorporar recuperación y reintentos conservadores de una sola operación.
- [x] Identificar handlers existentes y brechas de identidad/conciliación.
- [x] Definir contratos de superficies, preview, favoritos y nombres.
- [x] Implementar T0–T6 en un pedido de programación para Claude.
- [x] Efectuar sólo las comprobaciones autorizadas y registrar sus resultados.
- [ ] Confirmación del usuario de apariencia y comportamiento real pendiente.

### Entrega de programación

Base: HEAD `a67c8e66`, sin commit ni publicación. Tipo **Mediana**, inferido de correcciones funcionales y pequeñas adiciones: 0.9.0 → **0.10.0**. `version.json` queda con `release: null`, paso previo a compilar una entrega; no hay instalador ni manifiesto publicado.

| Requisito | Estado | Dónde |
|---|---|---|
| C0 | Implementado | `gateway_runtime.rs` (política), `controller/connection.rs` (suspensión), `Api::get_once` |
| H01 | Implementado | `ConnectionPhase`, `GatewayEvent::Connection`, `draw_connection_notice` |
| H02 | Implementado | `set_gif_favorite` único para catálogo y chat; menú y estrella en `media_card` |
| H03 | Implementado | `ui/text_viewer.rs`, `controller/attachments.rs` |
| H04 | Implementado | `reaction_tooltip.rs` |
| H05 | Implementado | `controller/outgoing.rs`, `draw_pending_messages`, `draw_send_failure` |
| H06 | Retirado | Sin cambios de voz, por instrucción del usuario |
| H07 | Implementado | `core/diagnostics.rs`, `app/diagnostics.rs`, `platform::reveal_file` |
| H08 | Implementado | `ColorEditor` en `appearance.rs`, `overlay.rs` |

Política de recuperación resultante: la tarea del Gateway no termina al agotar el presupuesto; queda detenida conservando Resume, límites y suscripciones, y sólo sale con una petición explícita ofrecida tras el cooldown. La apertura inicial no tiene intento automático; una sesión establecida tiene uno, y sólo otra sesión estable 60 s gana el siguiente, de modo que una conexión intermitente no puede formar un bucle. Agotar el presupuesto ya no bloquea `SafetyGate`: esa ruta forzaba volver a iniciar sesión y limpiaba borradores.

Ajustes respecto al plan:

- La navegación también se aplaza sin conexión, aunque C0.7 no la nombra: abrir un canal muestra la caché y carga su página una vez al sincronizar; cambiar de servidor informa la falta de conexión. Las mutaciones explícitas se rechazan con el mismo aviso. Voz queda fuera, para no tocar H06.
- En resultado desconocido el enlace dice «Click aquí para comprobar si se envió» y no el texto de reintento, porque esa acción no hace un POST.
- Hay que añadir la feature `Win32_UI_Shell_Common` de `windows`; `crates/app/Cargo.toml:42` no la tenía. No cambia `Cargo.lock`.

Limitación conocida: la conciliación por GET sólo identifica por nonce, y Discord puede omitirlo fuera de la respuesta del POST y de `MESSAGE_CREATE`. En ese caso la comprobación termina en «sigue sin confirmar» aunque el mensaje exista; un `MESSAGE_CREATE` posterior sí lo confirma. Por eso nunca se usa como prueba para reemitir.

Comprobaciones reales: `cargo check --workspace --all-targets --locked --offline` en debug, y `cargo check` y `cargo build --workspace --release --locked --offline` en release. Después, con autorización del usuario, `cargo test` de `notdiscord`, `notdiscord-core` y `notdiscord-media-addon`, sin cuentas ni cliente real; las pruebas de Gateway usan un servidor local sintético.

| Crate | HEAD original | Con esta tanda |
|---|---|---|
| `notdiscord` | 636 ok, 29 fallan | **680 ok, 0 fallan** |
| `notdiscord-core` | 161 ok, 1 falla | **163 ok, 0 fallan** |
| `notdiscord-media-addon` | no comparado | 38 ok |

Los 30 fallos del original se contrastaron por nombre contra una copia limpia de HEAD; esta tanda no añadió ninguno.

**Diagnóstico de esos 30: resuelto.** La suite completa pasa: **881 ok, 0 fallos, 5 ignoradas** (680 app, 163 core, 38 addon), en paralelo y en serie. Las 5 ignoradas requieren runtime externo o son soaks largos. Causa de fondo: la suite no se ejecutaba por la restricción de pruebas (los documentos de 0.8 y 0.9 lo registran), así que las pruebas quedaron atrás de cambios deliberados.

Fallos reales del producto, corregidos (2):

- Con un menú abierto, la rueda desplazaba el historial.
- El micrófono no se podía ajustar desde la tarjeta propia: la acción `participant-input-volume` existía y estaba probada en el controlador, pero `draw_call_participant_menu` salía antes de dibujar el control. Ahora muestra «Volumen de tu micrófono», 0–200 %.

Las otras 28 pruebas se actualizaron, sin cambiar producto:

| Grupo | Evidencia |
|---|---|
| `access::…member_list` (1) | El bundle web original exige que el rol @everyone vea a nivel de servidor y que ningún overwrite niegue `VIEW_CHANNEL`. El producto ya hacía eso; la prueba estaba mal. Se renombró |
| Helper `maybe_painted_text` y panel de reacciones (1) | No entraba en `Shape::Vec`, y miraba el primer frame invisible de un panel nuevo |
| Listas de miembros (5) | El código exige un enlace confirmado |
| Menciones (2) | `@time` y `@game` se completan antes que las personas |
| Botones y menús (3) | Comportamiento vigente de «Mensajes anteriores», menú de borrados y filtro «Conectados» |
| Voz en controlador (5) | Comentarios del código, `host.rs`, y el cliente original para `op 19` de espectador |
| Voz en UI (6) | Contrato de 0.8/0.9 y comentario «Drawing a snapshot cannot tear down reception»: dibujar no libera recepción, un decoder nuevo conserva la elección y sólo un anuncio conocido retira una fuente |
| IME (1) | El composer toma el foco al confirmar la composición; la prueba ahora exige que nunca esté en otro sitio |
| `pending_local…` (1) | Diferencia sub-píxel entre ancho de tarjeta y botón |
| Galería, panel y composer (3) | Ver abajo |

Tres pruebas aceptan el comportamiento actual, pero es una decisión de diseño que conviene revisar a ojo:

- La galería usa hasta 4 columnas a cualquier ancho, también a 370 px (cada celda queda en unos 80 px). La prueba se renombró.
- El panel de llamada mide 137 px aunque el presupuesto sea 80, para no recortar sus controles.
- El composer reserva la barra de scroll de los emojis aunque no desborde: 18 px de más a 700 px.

Límite de esta verificación: las pruebas de voz sólo comprueban la interfaz y el controlador con dobles sintéticos, y varias se reescribieron siguiendo los contratos documentados. Demuestran coherencia con esos contratos, no que la voz funcione con Discord real.

Cubierto por pruebas que pasan: política de un intento y detención del Gateway, coalescencia de clics, conciliación por nonce en ambos órdenes, clasificación de fallos, decodificación y límites del visor, editor de color y cierre ordenado. También el panel de reacciones, el orden de filas locales y el control de micrófono propio. Sin confirmar: apariencia, DPI, Explorador, portapapeles, CDN real y comportamiento con Discord real. Los criterios de aceptación visuales y de servicio siguen pendientes del usuario.

### Preparación de publicación — 1 de noviembre de 2026

El usuario autorizó integrar en `NotDiscord` y publicar instalador/manifiesto después del push de `9e1519a6a5889b67c122f961f913f8faee1802f2`. Se conserva la clasificación Mediana y la versión 0.10.0: aún no había una release de esta versión. La instalación y los accesos locales no se modifican.

La revisión previa detectó dos brechas de C0 no cubiertas por la admisión inicial: futuros HTTP ya lanzados podían esperar el semáforo y emitir después de perder sincronización; y el lote de ACK podía entregar hasta dieciséis operaciones al recuperar conexión. Se detuvo el primer build antes de empaquetar para cerrar esas rutas. La corrección liga el trabajo HTTP a la generación del transporte, revalida antes de emitir e invalida las esperas anteriores sin reenviarlas; el backlog de ACK se retira al suspender y queda como máximo uno en vuelo. Una petición ya emitida conserva su confirmación o resultado desconocido y las barreras de operación/rate limit.

Se actualizan los README y el contrato de distribución para 0.10.0. El runtime se prepara en un staging nuevo con inventario de 31 componentes x64, preservando vendor por hash y loaders del checkout. Se compilará la revisión definitiva con `NOTDISCORD_BUILD_ID` y manifiesto `release: null`; después se generará Setup y sólo se anunciará tras verificar su asset remoto. Las nuevas regresiones se añaden como fuentes; en esta publicación no se ejecutan pruebas, cliente, cuentas, dispositivos ni instalador. Los resultados históricos de la entrega de Claude no se convierten en resultados de esta revisión.

### Instalador 0.10.0 publicado — 1 de noviembre de 2026

Publicación autorizada por el usuario y verificada en GitHub: [NotDiscord 0.10.0](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.10.0), pública, sin draft/prerelease, título exacto y un único asset. La etiqueta resuelve a **`aa5b31239b57be9a607bcd5c4085d87d06f67755`**, la revisión compilada y el build ID. Incluye el commit de Claude `9e1519a6` y el cierre de las dos brechas de C0 descritas arriba.

Comprobaciones propias de esta publicación:

- `cargo check --workspace --all-targets --release --locked --offline -j1`: correcto en **31,10 s**, sin ejecutar destinos.
- `cargo build --workspace --release --locked --offline -j1`: correcto en **7 min 09 s**. Se conservaron advertencias existentes; no hay errores de compilación.
- Las **429 fuentes/entradas de compilación** conservaron sus hashes durante check/build; manifiesto agregado SHA-256 `8c88b3edb0e3efa0f7f105913e2209ce44dd78f834de459c85e695c0e19cf80b`.
- Runtime nuevo de 31 componentes x64 y payload de **41 archivos** verificados por inventario/hashes; EXE y addon del payload idénticos a los compilados. Inno Setup compiló sólo Setup, excluyendo datos, cachés y lanzador CMD.
- Tamaño y digest del asset remoto coinciden con el instalador local; la etiqueta también fue resuelta y contrastada con la revisión compilada antes de anunciar el manifiesto.
- Las nueve regresiones añadidas para la suspensión de HTTP/ACK se compilaron como parte de todos los destinos; **no se ejecutaron**. Tampoco se ejecutaron cliente, instalador, cuentas ni dispositivos. La apariencia y el funcionamiento real siguen pendientes del usuario.

| Artefacto | Identidad verificada |
|---|---|
| Instalador | `NotDiscord-Setup-0.10.0.exe`, **118 867 311 bytes** |
| SHA-256 Setup | `f0345ae8c3bfaf978a68796cfadc783da325d1749696a8b46b8485f9ac4877d9` |
| SHA-256 EXE | `861aef50d274c7aaf49ee76c369a1395d01ead3acadb80f999ac09ca9e6c1d29` |
| SHA-256 addon | `0ab369b57973d1837b68b34881f3168e1a68b2a94e7d3441c7a1607131cc75d5` |

Tras verificar el asset se incorpora su metadata a `version.json` de la rama `NotDiscord`. El instalador contiene el manifiesto `release: null` de la compilación; no se reconstruye para introducir su propio hash. Logs, inventarios y recibos de publicación quedan en `.tools/temp/current/release-0100/`, fuera de Git y del instalador. `evidence/` sigue fuera del commit.

NotDiscord+ conserva sus 38 archivos de distribución y la rama `d35c06ac7e824c17198791b3d9208f732bc8be81`; legacy permanece en `637d41f897a0e435c3d7e26d1c400afd6a640d0e`. Se contrastaron antes/después. No se modificaron instalaciones, datos ni accesos directos de uso del usuario.

### Huellas de fuentes del corte de planificación

Rutas relativas a `NotDiscord-Rust/`; hashes SHA-256. Las líneas citadas corresponden a la base indicada y deben revalidarse si avanza la rama.

| Archivo | SHA-256 |
|---|---|
| `crates/app/src/controller.rs` | `de8b3fa1a93abdd6fd32cdd66a3828383e4901da49a34bdbe222f744c5398759` |
| `crates/core/src/gateway_runtime.rs` | `692df6dc9dc929fa978496f012701809501aa3c650c672560a171e343336e903` |
| `crates/core/src/api_operations.rs` | `545feee8bd09a3d190c8722eb2c844d105cd0439d49b1e625c45b5c90201e909` |
| `crates/app/src/ui/chat.rs` | `f1d68e0234429b802ed0fed683772a0171795c887900bad2e131de8ffae4e7d6` |
| `crates/app/src/ui/overlay.rs` | `668ea02dd6df1c6bf34414f5b294324a3358d534be4ea9e7a84169a45366251a` |
| `crates/app/src/ui/appearance.rs` | `8dd2e9b9e5b494f5dbf9f7acf5f584088c8d804d6a5551c455f3211564daf5f1` |
| `crates/app/src/diagnostics.rs` | `7877500e326c48ea919d31197713e0fc6ceb08613d121ac2de2c1f447b01edfd` |
| `crates/core/src/diagnostics.rs` | `3fa606d260ec91f5ecb4a0a8b31f08074f5b7ea93134c527c9b8f9181de32d98` |
