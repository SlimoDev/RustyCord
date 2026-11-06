# Plan de llamadas, micrófono, presencia y navegación — iniciado el 4 y actualizado el 6 de noviembre de 2026

Estado vigente al **6 de noviembre de 2026**: **0.12.0 publicada por autorización posterior del usuario**, con instalador y manifiesto verificados. El apartado 9 conserva el corte de planificación, el 10 la entrega local y el 11 la publicación; la [nota 0.12.0](llamadas-presencia-navegacion-0.12.0.md) registra T7–T13 y sus límites. La ejecución anterior de T1–T6 conserva sus resultados en el apartado 8 y la [nota de implementación](llamadas-presencia-navegacion-0.11.0.md). **C5 sigue abierto:** diagnóstico acotado implementado, causa física y audibilidad remota sin acreditar.

Los apartados 1–7 conservan el corte histórico de planificación, sus restricciones de entonces y las huellas de la base. Sus frases «no realizado» o «pendiente» no describen la ejecución posterior, registrada en el apartado 8 y la nota enlazada. La publicación en GitHub se autorizó posteriormente; las pruebas con cuentas/dispositivos y la instalación siguen fuera del alcance ejecutado.

## 1. Base, alcance y evidencia

- Repositorio: raíz del checkout; aplicación: `NotDiscord-Rust/`.
- Variante principal, rama `NotDiscord`, HEAD `865b2f11069fadcd40f40bc634e7162ea7694f3e` (NotDiscord 0.10.1). No se cambia de rama.
- Corte: fuentes locales revisadas el 4 de noviembre de 2026. Las referencias de líneas de este documento corresponden a ese corte; sus huellas figuran al final.
- Cambios previos conservados: README de la aplicación, plan del 31 de octubre, skill del repositorio y sus referencias; contrato conservador y `evidence/` sin seguimiento. No se atribuyen esos cambios a esta planificación.
- No se halló `AGENTS.md` dentro del repositorio en la búsqueda realizada. Se leyeron la skill instalada, su mapa/plantilla, `docs/plan-skills.md` y el contrato conservador local.
- Tipo propuesto: **Mediana**, por corregir recorridos funcionales de llamadas y añadir su presentación cuando el usuario no participa. Según `docs/plan-skills.md:145–170`, la propuesta es **0.10.1 → 0.11.0**. Es una inferencia para una futura entrega; no se cambia ningún manifiesto ni se renumera una versión publicada.
- Este pedido incluye voz: la retirada de llamadas del plan histórico del 31 de octubre no excluye estos requisitos nuevos. No se extiende el alcance a Plus ni a legacy.

Las dos capturas son evidencia aportada por el usuario, no reproducciones realizadas por el agente. La primera muestra el botón textual de miembros en una fila distinta de «Volver a participantes» y «Ajustar». La segunda muestra el registro RAM y «Volver al chat», sin acreditar por sí sola la secuencia de navegación. El resto son incidencias reportadas. El usuario aclaró durante el análisis que el audio que se pierde es **el micrófono**; el posible efecto de la cámara sigue siendo una hipótesis.

### Requisitos activos

| ID | Comportamiento solicitado | Tanda |
|---|---|---|
| R1 | Al abrir un grupo con llamada ya iniciada, ver la zona de llamada, sus participantes conocidos y «Unirse», aun sin estar conectado | T1 |
| R2 | Conservar el micrófono al iniciar, detener o cambiar pantalla compartida; comprobar también cámara | T2 |
| R3 | Cambiar visualmente el estado propio inmediatamente, incluida la barra de miembros, manteniendo la operación real pendiente de confirmación | T3 |
| R4 | En cámara/pantalla ampliada, una fila con el icono de miembros, «Volver a participantes» y «Ajustar» | T4 |
| R5 | «Volver al chat» desde el registro RAM regresa a la conversación correspondiente | T5 |
| R6 | Salir de llamada se refleja rápidamente en la previsualización; las salidas ajenas se reflejan al recibir su evento | T1 |

«Estados» se interpreta como En línea/Ausente/No molestar/Invisible. No se incluye aquí rediseñar biografía, estado personalizado o actividad de juegos, aunque deben conservarse. La respuesta visual inmediata es local; no promete que el servidor o los demás clientes hayan aplicado todavía la elección.

### Límites y entrega

Esta solicitud autoriza revisar fuentes y escribir el plan. No se ha compilado, ejecutado pruebas, abierto el cliente, utilizado cuentas, cámara/micrófono/pantalla, instalado herramientas ni publicado. Tampoco se modifican fuentes del producto, datos, configuración, skills o instalaciones.

Las restricciones de ejecución de notas de entregas anteriores son contexto histórico; no se convierten en una prohibición perpetua. Al continuar con programación se aplicarán la autorización y la skill vigentes, sin inventar una aprobación intermedia del plan. Una futura entrega instalable/publicación sólo se realizará dentro del alcance que entonces esté autorizado.

## 2. Hallazgos y prioridades

P1 indica fallo funcional de llamada; P2, interacción o presentación. No es una clasificación de seguridad.

Abreviaturas de fuentes: `controller/` y `ui/` pertenecen a `crates/app/src/`; `core/` a `crates/core/src/`; `media-addon/` a `crates/media-addon/src/`. Las funciones y líneas citadas son de la copia local observada.

### H1 — P1 — La llamada observada depende de haber recibido CALL_* en esta sesión

**Confirmado por lectura estática:**

- `ui/voice.rs::call_panel_visible:4–8` exige voz local conectada/conectando; para el usuario fuera de la llamada existe otra superficie, `draw_incoming_call_in_chat:801–840`.
- Esa superficie sí acepta una llamada sin timbre si tiene participantes; no hace falta volverla a diseñar ni unir al usuario para mostrarla.
- `controller/incoming.rs::incoming_call:1932–2043` crea el registro desde CALL_CREATE/CALL_UPDATE y lo retira con CALL_DELETE. `incoming_media_dispatch:1865–1930` actualiza registros ya existentes; un VOICE_STATE_UPDATE aislado no crea una llamada privada ausente.
- `core/catalog.rs::ready_catalog:42–116` proyecta servidores, conversaciones y relaciones, sin llamadas privadas. `core/gateway.rs::ready_metadata:586–654` conserva voz de servidores, pero no una semilla de llamadas privadas.
- `core/gateway.rs::GatewayCommand:65–103` no tiene un comando tipado de descubrimiento/sincronización de llamada privada. `Controller::select_channel_at:1230–1337` navega/carga conversación e historial, sin ese descubrimiento.
- `decorate_dms:845–850` deriva el indicador lateral de la existencia del registro, incluso si éste ya no contiene participantes.

**Precondición e impacto:** si una llamada comenzó antes de la sesión o no llega CALL_* al abrir el grupo, la UI no tiene de dónde presentar «Unirse». Ya existe soporte para llamadas sin timbre, mitigación que debe reutilizarse.

**Certeza del incidente:** falta de cobertura de estas rutas confirmada; no está demostrado qué datos envió Discord en la sesión reportada. No se afirma que READY siempre incluya llamadas ni que un endpoint concreto exista.

**Propuesta:** separar llamada observada, invitación/timbre y conexión local; completar el descubrimiento dirigido del grupo visible y usar un único criterio de actividad para previsualización e indicador lateral. No convertir el último mensaje de tipo llamada en prueba de que sigue activa.

### H2 — P1 — Al salir quedan datos de previsualización y puede demorarse su publicación

**Confirmado por lectura estática:**

- `controller/voice.rs::stop_voice_internal:1512–1618` cierra recursos y estado de la conexión local, pero no retira al usuario de `incoming_calls`; `refresh_call` sigue decorando los DMs con ese registro.
- `voice_dispatch:2647–2664` primero modifica las llamadas observadas mediante `incoming_media_dispatch`. El acuse de salida propio puede retornar en 2664 sin `refresh_call/publish`; otra salida propia puede retornar en 2750 sin publicar si ya no hay `voice_channel`. El cambio puede quedar pendiente hasta otra publicación.
- `publish:525–548` marca el snapshot como sucio; su envío se realiza en `tick:2403–2420`, con coalescencia de 60 ms. El actor espera a que termine `act/dispatch` (`controller.rs:250–264`).
- Después de `refresh_call`, la salida espera secuencialmente StreamDelete, si aplica, y VoiceState. `GatewayHandle::send_at_game_epoch:239–252` puede esperar hasta 10 s por orden. Mientras el handler sigue esperando, el tick no puede publicar.
- El criterio lateral «hay registro» y el del panel «timbre o participantes» difieren (`incoming.rs:845–850` frente a `ui/voice.rs:814–820`).

**Impacto y certeza:** son rutas alcanzables de datos/publicación retrasados, no una medición de la demora del usuario. El intervalo normal de 60 ms no demuestra una espera de varios segundos. La política conservadora no exige esperar a la red para dibujar el cierre local.

**Propuesta:** proyectar inmediatamente la salida propia, preservar participantes remotos y confirmar después; sacar la espera de señalización del bucle del actor mediante una operación de cierre identificada. Publicar también las mutaciones de llamadas observadas aunque el handler retorne temprano. Mantener la barrera de salida antes de conectar a otra llamada.

### H3 — P1 — Pérdida de micrófono al compartir: causa aún no confirmada

**Recorrido revisado:**

- `controller/voice.rs::try_share:2982–3002` inicia startShare en el mismo host, con sesión de transporte Go Live separada.
- `media-addon/host.rs:854–890,1054–1095` crea esa sesión sin cerrar intencionalmente la voz principal. Cámara usa la sesión principal (`voice_command:2289–2356`).
- `Host::ready:1307–1325` conserva vídeo en `s.streams` y construye inicialmente parámetros con audio + vídeo. En startCamera (`2340–2343`) y share_capture (`2512–2530`) las actualizaciones de `streamParameters` se construyen sólo desde los streams de vídeo.
- `Host::apply_audio:447–458` controla entrada/salida global del motor; `configureAudio:2151–2164` puede reconfigurar la entrada y reiniciar PTT/actividad. Esta ruta es relevante si se guardan ajustes; no se ha demostrado que iniciar pantalla por sí solo la invoque.
- `Tag::MicLevel:1822–1856` alimenta detección y puerta de transmisión; `transmit_gate:474–487` protege mute, cifrado y PTT.
- `Session::send_mute:219–220` mantiene mudas las sesiones de stream. Es una separación deliberada: el micrófono debe seguir saliendo por la llamada principal.

**Hipótesis que deben discriminarse:** actualización de parámetros que sustituya la configuración de audio; interferencia del motor global entre captura de pantalla y micrófono; pérdida de callbacks de nivel o desajuste de PTT/VAD tras configurar/renegociar. La semántica nativa de sustitución frente a mezcla de parámetros **no se ha verificado**. La lista sólo de vídeo no demuestra por sí sola el origen del fallo.

**Mitigaciones a conservar:** sesiones distintas, selección explícita, tokens, DAVE, barreras de mute/deaf y aislamiento del receptor. No abrir el micrófono de Go Live ni desactivar cifrado para hacer audible el audio.

**Propuesta:** primero registrar y contrastar la secuencia con dobles y contadores existentes; corregir el punto probado, compartiendo la construcción de parámetros cuando corresponda. No dar por resuelto el incidente porque cambie el medidor local.

### H4 — P2 — El estado visual propio espera todo el recorrido remoto

- `ui/shell.rs::account_menu:2179–2187` envía la elección sin guardar una proyección visual inmediata.
- `controller/presence.rs::start_presence_change:108–139` marca la operación pendiente, pero no cambia la presentación.
- `execute_change:36–48` espera PATCH y luego el envío de presencia por Gateway; `finish_presence_change:141–163` aplica después el valor confirmado.
- `project_self:15–31` ya proyecta el estado confirmado a presencias, miembros de servidor/grupo, DMs y perfil. `Controller::flush_snapshot:541–543` lo vuelve a aplicar.
- `ui/mod.rs:834–849,1075–1090` conserva una barrera de política de notificaciones; reutilizar `self_presence` como optimista podría afectar sonidos, privacidad o la proyección de un snapshot viejo.
- `change_presence:100–106` ya limita una escritura en curso y una elección posterior coalescida, mitigación a preservar.

**Certeza:** espera visual confirmada por fuentes; no se midió la latencia. La confirmación HTTP de ajustes y el envío de presencia son resultados distintos. El ACK de envío del Gateway no prueba por sí solo que otro cliente ya muestre el estado.

**Propuesta:** mantener estado confirmado e intención visual separados, con una sola proyección de presentación para todas las superficies del usuario propio. La elección reciente no debe retroceder por una respuesta de una elección anterior.

### H5 — P2 — La cabecera ampliada está partida por estructura de layout

`ui/voice.rs::draw_call_in_height:69–72,117` reserva/dibuja el menú textual de miembros por separado. `draw_expanded_call_video:162–185` crea después otra fila para volver/ajustar. `draw_call_invite_menu:1553–1559` utiliza texto; ya existe `shell::Icon::Members:349–355,425`.

**Certeza:** estructura confirmada y coincidente con la captura 1. **Propuesta:** una cabecera compartida para la ampliación con el icono existente y los dos botones, manteniendo menú, accesibilidad, zoom, desplazamiento y controles inferiores.

### H6 — P2 — El registro se abre con la acción que elimina su conversación

`ui/chat.rs:191–201` envía `view/history`, mientras `Controller::act:1656–1677` trata `view` como navegación de inicio y borra guild/canal/mensajes/permisos. La acción específica `history:1926–1934` sí conserva el contexto y carga el registro RAM. El retorno (`1679–1698`) cambia la vista y refresca mensajes, pero no puede recuperar un canal ya borrado.

**Certeza:** recorrido confirmado, sin reproducción. El botón de retorno existe (`ui/chat.rs:122–124,573–576`); no basta con cambiar su etiqueta. **Propuesta:** usar la acción específica al abrir, validar su canal y conservar/restaurar el contexto de navegación. El registro seguirá siendo local y efímero.

## 3. Instrucciones conservadoras para quien programará

Referencia leída: `docs/desarrollo-conservador.md`, consolidación del **4/11/2026**, base `865b2f11…`, SHA-256 `a17857b348e377cd1aa5d0c0e776c80e78da07b5dbd6cf927a2352bc50a7f6ea`. Estos requisitos se transmiten aquí para que el plan pueda enviarse por separado.

1. **Propiedad y admisión.** Toda demanda/operación pertenece a cuenta y generación. Para llamadas añadir canal, instancia observada y token del intento; para presencia, revisión de intención; para navegación, canal y revisión de vista. La recuperación sigue teniendo un único propietario compartido con reintentos de mensajes. No crear un coordinador paralelo desde voz, presencia o UI. Admitir lecturas opcionales sólo sincronizado; revalidar después de esperas y justo antes de emitir.
2. **Un intento, sin ráfagas.** Coalescer clics y mantener una operación activa por recorrido. La apertura inicial no encadena intentos; tras una pérdida conservar como máximo el intento automático de recuperación vigente y detenerse si falla. Respetar Retry-After, límites globales/de ruta e Identify. Los 30 s de cooldown y 60 s de estabilidad son políticas implementadas que deben revalidarse, no límites oficiales ni nuevos requisitos del usuario.
3. **Pasos obligatorios.** Hello, Identify/Resume, READY/RESUMED, heartbeats y pasos imprescindibles de voz/cifrado siguen siendo parte del protocolo. Cerrar captura local y procesar eventos ya recibidos no requieren una nueva conexión ni una consulta. No condicionar el repintado a terminar un cierre remoto.
4. **Confirmaciones e incertidumbre.** No confundir una cola local, flush del Gateway, PATCH confirmado y estado remoto observado. Timeout/cancelación no demuestran rechazo ni que el trabajo haya terminado. Si una escritura pudo salir, marcar resultado desconocido; no reemitir ni lanzar la siguiente escritura automáticamente para encubrirlo. Correlacionar los resultados y preservar confirmaciones tardías válidas.
5. **Presencia serializada.** Conservar una escritura y una elección posterior coalescida. Sólo avanzar normalmente tras resultado conocido y admisión vigente; un fallo, 429 o resultado desconocido detiene la secuencia. Ofrecer una comprobación explícita y acotada cuando proceda. No drenar elecciones al reconectar ni interpretar un eco antiguo de estado como confirmación de la intención reciente.
6. **Operaciones y wrappers.** Conservar `Operations.Unknown`, RateLimits y los transport tickets; no reconstruir Api ni variar el cuerpo para eludirlos. `Api::request_attempts:463–523` no repite automáticamente PATCH, pero puede repetir GET hasta tres intentos ante ciertos 429. Nuevas comprobaciones dirigidas deben usar `get_once` o un contrato equivalente de una emisión; no heredar retries internos por accidente. En presencia, conservar errores tipados antes de convertirlos a texto para distinguir rechazo, rate limit e incertidumbre.
7. **Generaciones y arranque.** Cancelar trabajo viejo aún no emitido; lo ya emitido conserva la posibilidad de confirmar. Capturar semilla y demanda visible antes de clonar Api/admitir trabajo. READY_CATALOG puede preceder a Synchronized: conservar demanda y crear un único trabajo fresco al sincronizar. No drenar backlog ni renovar tickets cerrados. Aplicar también al descubrimiento de llamadas y a catálogos parciales.
8. **UI y cachés.** No HTTP/Gateway por frame, hover o repintado. Obtener llamadas por eventos o demanda explícita de la conversación visible, compartida y acotada. Un TTL o volver a dibujar no rearma un fallo. Mantener campos ausentes como desconocidos y distinguir snapshots completos de deltas; un roster incompleto no autoriza retirar ausentes.
9. **Privacidad y efectos.** La proyección visual no modifica credenciales, permisos, conectividad ni política confirmada de notificaciones/juegos. Invisible no equivale a desconexión global. Mantener aislamiento de cuentas, barreras Authentication/Verification/Protocol, permisos de conexión/vídeo y selección explícita. El host no recibe tokens/cookies de cuenta; conserva únicamente las credenciales de medios previstas por su contrato.
10. **Medios y datos.** Mantener DAVE, mute/deaf/PTT, prueba de micrófono aislada, selección de fuente y límites de streams, resolución, tiempo y memoria. No grabar audio ni añadir dumps de cuentas. Diagnóstico sólo de fases, conteos y correlaciones efímeras, sin IDs privados, mensajes, dispositivos, URLs firmadas ni contenido de pantalla. Conservar borradores, scroll, cachés válidas y configuraciones.
11. **Recuperación.** La salida visual no habilita una llamada nueva hasta resolver la barrera de salida anterior; el resultado tardío no cierra su reemplazo. El registro RAM y la cabecera ampliada son locales: no requieren tráfico nuevo salvo la carga ordinaria del chat, que debe respetar los gates. No introducir migraciones/persistencia privada ni reiniciar la cuenta para ocultar fallos.

## 4. Tandas ejecutables

Orden recomendado: **T1 → T2 → T3 → T4 → T5 → T6**. T3 y T5 no dependen funcionalmente de T2, pero se propone una implementación secuencial. No se requiere delegación. Todas las tandas empiezan **pendientes**.

### T1 — Llamada observada, descubrimiento y salida rápida (R1/R6)

**Objetivo:** una misma autoridad para actividad/participantes en previsualización y barra lateral, independiente de que el usuario participe o reciba timbre.

**Superficies:** `core/gateway{,_runtime}.rs`, proyección READY si corresponde; `controller/incoming.rs`, `voice.rs`, `private_calls.rs`; modelos y `ui/voice.rs`.

**Trabajo:**

1. Reutilizar los registros acotados actuales de llamada observada; separar conocimiento de llamada, invitación/timbre y transporte local. Modelar conocido activo/finalizado/desconocido o desactualizado, completitud y revisión. La expiración de una invitación no finaliza la llamada del grupo.
2. Incorporar una semilla privada sólo si el payload realmente disponible la contiene. Resolver usuarios desde metadata mínima ya recibida. Campo ausente no es lista vacía. Reutilizar los límites actuales de 32 llamadas y 25 participantes por registro, o justificar explícitamente cualquier cambio; representar truncado/parcial.
3. Completar el descubrimiento al abrir un grupo ya activo. **Prerrequisito técnico de esta tanda:** verificar el mecanismo/payload de consulta o sincronización con evidencia primaria/artefactos permitidos, porque el checkout no lo implementa. Este análisis no ha acreditado una ruta HTTP ni un opcode; no inventarlos. Si hace falta estudiar un cliente compilado, aplicar la skill REA dentro del alcance autorizado.
4. Exponer ese mecanismo con comando tipado validado, una demanda por cuenta/generación/canal y una operación activa. Priorizar el grupo visible; no escanear todos los grupos. Si la demanda llega antes de sincronizar, retenerla sin clonar un Api cerrado. Si falla, detenerse y ofrecer actualización explícita; sin polling. Una respuesta vieja no puede sobrescribir un CALL_UPDATE/CALL_DELETE posterior.
5. La previsualización muestra «Llamada en curso», roster conocido y «Unirse». No muestra «Conectado» ni crea host, micrófono, stream o receptor al dibujarse. Sólo una acción explícita de unirse conecta; unirse a una llamada existente no vuelve a llamar/invitar a los miembros.
6. Al salir localmente, guardar antes la identidad de llamada y crear una intención de salida. Cerrar la captura/recursos propios y proyectar su ausencia en el próximo frame; conservar a los remotos. Si no quedan remotos, no mantener una tarjeta propia como si siguiera conectado. La información aún incierta conserva una indicación coherente, sin afirmar fin remoto.
7. Llevar la señalización pendiente a una tarea/completion de cierre que deje libre al actor. Mantener un único cierre activo, su token independiente del nuevo `voice_attempt`, el orden requerido y los acuses. Un error no produce un nuevo envío ciego. Documentar qué pasos de liberación son obligatorios aun si otro paso falla; no convertirlos en ráfagas ni omitir el cierre necesario. No limitarse a `spawn` sin dueño ni confirmación.
8. Publicar cambios del registro antes de todos los retornos tempranos pertinentes. No forzar todos los snapshots a cada frame: utilizar una notificación/proyección inmediata acotada para la intención y publicaciones al cambiar datos.
9. Definir un único predicado de actividad para panel/indicador: un registro vacío finalizado no deja el icono activo, un registro parcial no acredita fin, y los eventos explícitos conservan prioridad. Mantener tombstones, anuncios independientes de cámara/pantalla y guardas frente a callbacks tardíos. Una instancia nueva no hereda anuncios ni una salida pendiente de la anterior.

**Aceptación propuesta:**

- Dos personas ya en llamada; una tercera abre el grupo antes/después de READY: ve roster y Unirse sin adquirir audio. Conectar luego no envía ring.
- Timbre ausente/expirado/rechazado no elimina la llamada si quedan participantes. CALL_DELETE y un roster completo vacío sin timbre retiran la actividad.
- Salir actualiza en el siguiente frame disponible y sin esperar un ACK demorado; los remotos restantes siguen visibles. Una salida remota se refleja en el primer snapshot publicado tras procesar su evento, sin esperar otro mensaje.
- Cubrir salida con pantalla activa, StreamDelete lento, Gateway sin responder, varios clics, leave → join inmediato, cambio de grupo/cuenta, ACK viejo y CALLBACK tardío. Se conserva la barrera contra abrir el micrófono en una llamada nueva antes de cerrar la anterior.
- READY parcial, sincronización tardía y resultados invertidos no reintroducen participantes retirados ni disparan backlog. El presupuesto de consultas no crece por repintados.

**Dependencia de cierre:** R1 no se marcará resuelto sólo por mejorar la tarjeta: debe acreditarse de dónde obtiene la llamada que ya estaba activa.

### T2 — Continuidad del micrófono al publicar vídeo (R2)

**Objetivo:** mantener el audio de voz principal durante toda transición de cámara/pantalla.

**Superficies:** `controller/voice.rs`, `media.rs`; `media-addon/host.rs`, `session.rs`, `settings.rs`, `stats.rs` y sus dobles existentes. Depende del contrato de identidad/cierre de T1.

**Trabajo:**

1. Preparar una traza sintética ordenada de llamada conectada → selección → startShare/startCamera → negociación → captura → stop/error. Registrar qué sesión recibe cada orden nativa y qué cambios globales se aplican.
2. Distinguir cuatro evidencias: entrada/callbacks; elegibilidad VAD/PTT; puerta de transmisión/cifrado de la voz principal; contadores de salida. Reutilizar inputLevel/stats y emitir sólo cambios de fase o resumen acotado, sin grabaciones.
3. Determinar si `streamParameters` se mezcla o sustituye en la ABI efectiva. Si reemplaza, construir desde un helper común el descriptor completo de audio + vídeo negociados, con SSRC/bitrate/tipo propios de cada sesión. Evitar retirar audio al añadir vídeo. No modificar la lista sólo por sospecha.
4. Evitar que la sesión de pantalla o un preview cambie dispositivo, callback o puerta del micrófono principal. Si la evidencia señala reconfiguración de audio o reset de PTT/VAD, corregir propiedad y rearme por sesión/intención vigente. Una pérdida real de DAVE conserva el cierre seguro hasta recuperar cifrado.
5. El cierre/error/reemplazo de vídeo retira sólo su publicación. No desconectar/reconectar la voz principal ni habilitar el micrófono en la sesión Go Live como arreglo.
6. Si los dobles no discriminan la causa nativa, registrar esa limitación y el diagnóstico mínimo necesario para una comprobación real autorizada. No declarar solución funcional basándose sólo en que las órdenes retornan éxito.

**Matriz de aceptación propuesta:**

| Recorrido | Variantes mínimas | Resultado |
|---|---|---|
| Pantalla iniciar/parar/cambiar | Audio de pantalla activado/desactivado | El micrófono sigue por la voz principal |
| Cámara iniciar/parar/cambiar | Cámara sola y junto a pantalla; ambos órdenes | Sin pérdida de voz ni retirada cruzada |
| Voz | VAD y PTT; mute/deaf; volumen 0 y luego restaurado | Respeta los controles; no abre captura no solicitada |
| Error/transición | Fallo de vídeo, timeout, rekey DAVE, evento tardío | Fallo contenido en su fuente; sin reapertura insegura |
| Recepción | Persona que recibe hablando y recibiendo a la vez | Sigue la comunicación bidireccional |

Con fuente sonora sintética en un ensayo autorizado, los contadores de audio saliente principal deben progresar antes/durante/después del vídeo; la escucha de otro participante confirma entrega. Medidor/contador local por separado no prueba audibilidad remota. Comparar con voz sin vídeo, sin fijar umbrales de pérdida o latencia no medidos. El sonido de pantalla es una regresión a conservar, no el incidente confirmado.

### T3 — Estado propio visual inmediato y confirmación real (R3)

**Objetivo:** separar intención de presentación y estado confirmado, con coherencia en todas las superficies.

**Superficies:** `controller/presence.rs`, completions/errores en controlador, modelos/eventos; `ui/mod.rs`, `shell.rs`, miembros y perfil.

**Trabajo:**

1. Al aceptar el envío local del comando, guardar intención visual identificada por cuenta/generación/revisión. Dibujarla desde el siguiente frame y solicitar repaint. Si el canal de comandos falla, no dejar una intención pendiente imposible.
2. Mantener el valor confirmado independiente. No llamar al setter que aplica notificaciones, suspende RPC o altera privacidad únicamente para pintar el nuevo color.
3. Reutilizar/factorizar la proyección de `project_self` para la presentación; aplicar el mismo estado a avatar propio, perfil, presencias y fila propia de miembros de servidor/grupo **antes** de agrupar/ordenar las filas. Evitar cuatro implementaciones divergentes o clonar grandes listas en cada frame; cachear al cambiar intención o snapshot pertinente.
4. Conservar una escritura en curso y sólo la última elección siguiente. Su revisión visual es distinta de la revisión de petición. Una respuesta de online no elimina la intención más reciente idle. Clic idéntico pendiente no crea otra escritura.
5. Transmitir resultados correlacionados: confirmado, rechazado, rate limited, persistido con fallo de emisión Gateway y desconocido. Rechazo definitivo vuelve al último confirmado con aviso; incertidumbre mantiene una marca de «sin confirmar», sin simular rechazo ni éxito y sin reenvío automático.
6. Con PATCH confirmado, conservar ese dato aunque falle el flush del Gateway; no repetir PATCH para corregir sólo la emisión. La conciliación explícita, cuando sea necesaria, es una lectura acotada de una emisión; no se encadena por ecos discrepantes. No alterar las barreras de Unknown por cuerpo/cambio de estado.
7. Al perder sincronización, detener el avance de elecciones; no emitir al recuperar automáticamente. Mantener una intención visible no emitida o incierta identificada, con acción explícita válida; cambio de cuenta elimina sólo la proyección de esa cuenta.
8. La política confirmada de notificaciones y juegos sigue su propio orden. Un snapshot antiguo o notification-policy no pisa la intención reciente de presentación; la intención tampoco sobrescribe política confirmada.

**Aceptación propuesta:** online → dnd → idle con primera respuesta demorada, ambos órdenes HTTP/Gateway, eco viejo, error definitivo, timeout después de emisión, PATCH correcto con Gateway fallido, 429, pérdida/recuperación y cambio de cuenta. En todos los casos coinciden los indicadores propios del panel y lista de miembros; no hay escrituras paralelas ni reenvío al reconectar. No usar Invisible para determinar el estado de conexión.

Los estados ajenos se presentan al procesar su evento Gateway, con publicación y repintado sin esperar otra interacción. Sólo el estado propio elegido admite la proyección optimista; no se predice la presencia de otras personas.

### T4 — Una fila de controles en el vídeo ampliado (R4)

**Objetivo visual:** `[icono Miembros] [Volver a participantes] [Ajustar]   Nombre · Cámara/Pantalla`.

**Superficies:** `ui/voice.rs`, helper de iconos de `shell.rs`. Depende sólo del modelo de llamada estable; no requiere red nueva.

**Trabajo:** llevar menú de miembros y botones a una cabecera compartida por el modo ampliado. Reutilizar `Icon::Members` del control de mostrar miembros, con tooltip y nombre accesible «Miembros de la conversación», manteniendo su menú y las opciones de añadir a llamada. Volver conserva su acción de galería, Ajustar reinicia zoom/pan; no confundir esas acciones con mostrar miembros. Si una acción no aplica al contexto, conservar su condición actual.

Calcular altura/presupuesto desde esa fila real y evitar dibujar el menú dos veces. El nombre del emisor puede truncarse antes de separar los tres controles. A la anchura de la captura, los tres van en una fila. En anchuras mínimas o texto ampliado, usar una adaptación explícita sin solapamientos y sin partir arbitrariamente sólo el primer control; puede abreviarse una etiqueta con tooltip o permitirse desplazamiento horizontal de la barra.

**Aceptación propuesta:** cámara y pantalla remotas, nombres largos, DPI 100/150/200 %, zoom de texto y ventana pequeña. Controles en la misma fila cuando caben, vídeo/controles inferiores sin recortes, menú y Escape funcionales. Mantener elección de fuente, calidad, zoom, pan y privacidad.

### T5 — Abrir y retornar del registro RAM con contexto (R5)

**Objetivo:** usar el recorrido específico de historial local y restaurar exactamente la conversación de origen.

**Superficies:** `ui/chat.rs`, `controller.rs`, helpers de navegación/cache. Independiente de T2–T4.

**Trabajo:**

1. Cambiar el emisor del menú al action específico `history` existente. Validar canal/permiso de lectura según el contexto vigente; no reutilizar el reset de Home.
2. Conservar cuenta, generación, guild/canal y contexto del chat al abrir. Añadir un contexto explícito de retorno sólo donde el estado ya retenido no baste; no duplicar historiales ni restaurar un origen de otra cuenta.
3. Retornar mostrando mensajes de la caché legítima, no los registros de ediciones/borrados como mensajes vivos. Si hace falta una carga ordinaria, aplicar el gate y la demanda diferida existente; sin nueva carga al disponer de datos válidos.
4. Mantener borrador, adjuntos preparados, scroll/ancla y selección del chat de origen. Cancelar resultados de vistas reemplazadas. El clic de regreso viejo no debe redirigir una conversación seleccionada posteriormente.
5. Si el canal dejó de estar disponible, mostrar el motivo y la salida coherente; no recuperar permisos desde una caché ni abrir un canal vacío. Mantener el registro únicamente en RAM y su límite existente.

**Aceptación propuesta:** DM, grupo, texto de servidor y chat de voz; registro vacío/con entradas; abrir perfil desde registro y volver; sin conexión; navegación rápida/cambio de cuenta/canal eliminado. «Volver al chat» conserva origen y borrador; el caso vacío de la captura también funciona. Verificar la cadena real menú → acción → registro → regreso, además del botón aislado.

### T6 — Revisión integrada, validación y documentación

Depende de T1–T5. No añade funciones.

- Revalidar base/hashes y contratos afectados. Registrar causa finalmente probada del micrófono y mecanismo acreditado de descubrimiento; ajustar las hipótesis del plan.
- Revisar flujos completos de admisión, esperas, emisión, confirmación, cancelación y recuperación. Incluir arranque con catálogo parcial, consulta admitida antes de su primer poll, 429, timeout con tarea aún activa, confirmaciones invertidas y eventos de otra cuenta/intento.
- Preparar regresiones pertinentes en fixtures existentes. No añadir pruebas que sólo repitan el código del layout. Las comprobaciones visuales y de audio se eligen por sus riesgos reales.
- Cuando el pedido de implementación permita comprobaciones, compilar destinos afectados con dependencias bloqueadas; ejecutar sólo pruebas compatibles con ese alcance. Cliente, dispositivos, cuentas, probes nativos o servicios reales deben estar dentro de la autorización vigente.
- Actualizar documentación de las tandas con resultados exactos: revisión, compilación, tests ejecutados, casos manuales y pendientes. No reciclar resultados históricos de 0.7–0.10 como evidencia de este cambio.
- Si se autoriza distribuir: revalidar la versión propuesta; fijar fuentes/build ID, compilar esa revisión, verificar runtime/payload/Setup y asset por tamaño/SHA-256/tag. Anunciar el manifiesto después del asset; no reconstruir Setup para incluir su propio hash. Preservar Plus/legacy, instalación y datos fuera de alcance.

## 5. Recuperación y criterios de terminación

No hay migración de esquema de datos ni respaldo de cuentas previsto: observaciones e intenciones son efímeras. Una reversión de UI no revierte un PATCH aceptado por Discord. Ante fallo, conservar último estado confirmado, incertidumbre y barreras de operación; cerrar sólo recursos propios del intento afectado. Para revertir código, usar una revisión identificada que conserve los cambios ajenos, sin reset destructivo.

Se puede terminar la implementación de una tanda con límites de verificación documentados; no se puede presentar R1 como resuelto sin descubrimiento real acreditado, ni R2 como audible remotamente sólo por un test de órdenes. Si no se autoriza ensayo real, entregar el cambio revisable con esa confirmación explícitamente pendiente.

| Actividad | Estado en esta planificación |
|---|---|
| Revisar los seis requisitos, capturas y aclaración de micrófono | Hecho |
| Leer contrato conservador, fuentes y callers citados | Hecho |
| Identificar causas demostradas frente a hipótesis | Hecho |
| Preparar T1–T6 y su aceptación | Hecho |
| Modificar/compilar/ejecutar producto o tests | No realizado |
| Acceder a cuentas/dispositivos, instalar o publicar | No realizado |
| Comprobar formato del plan y estabilidad final de fuentes | Hecho; detalle al cierre |
| Implementar y confirmar comportamiento real | Pendiente |

No hay una decisión de usuario que bloquee terminar el plan. Quedan dos incógnitas técnicas para implementación: origen exacto del fallo de micrófono y mecanismo verificable para descubrir una llamada privada previa a la sesión. La aclaración «micrófono» ya está resuelta.

## 6. Huellas del corte

SHA-256 de las fuentes locales citadas y contratos. Rutas relativas a `NotDiscord-Rust/`. Revalidar al continuar; HEAD por sí solo no describe los documentos locales sin seguimiento.

| Archivo | SHA-256 |
|---|---|
| `crates/app/src/controller.rs` | `4d3cae45fe82e4d6e38ff340f62eec92d02ec71ca99a8df2962629d0e151334c` |
| `crates/app/src/controller/incoming.rs` | `e649b59d72273324bc13181eded78c7af8f6a484416c87eec6076853e66f40e8` |
| `crates/app/src/controller/presence.rs` | `26b54e93c9d94c607dff823abcd946aeda071ac2d686ef5df87f2682e7907e76` |
| `crates/app/src/controller/private_calls.rs` | `9c9c2281f2332f16d8d4867700da36524b149b7b301bc229a4cd5414559537c5` |
| `crates/app/src/controller/voice.rs` | `3f23adb6b3a45f823c69a83c63a4de2f668379eec2b44b327e597dd895508f9c` |
| `crates/app/src/controller/connection.rs` | `04c4bda39e0560e674dd8611a152783e322435c5905ff1340c47d6bbff2128e2` |
| `crates/app/src/ui/mod.rs` | `435ef72d2707efc70d8f8774a40c9ab47dc1ae728bbd5dcdf9ee5b14973d2fb7` |
| `crates/app/src/ui/chat.rs` | `8f2237ca4f9835aa6470fc75bde3cc8e30312f6b9548f139880d4d3a90b45ca6` |
| `crates/app/src/ui/shell.rs` | `6d8689c5ac913bc1520485e2ab3497826cec6e84c88f331d7525fccf82d6e282` |
| `crates/app/src/ui/voice.rs` | `33b390ac7d2980b73b370fb78827f110cbd3df8a27a182ad3ba671536d0feba1` |
| `crates/core/src/catalog.rs` | `d0118d52ded9484373264c938b70e78302ae97c5861742100a68c4516791c303` |
| `crates/core/src/gateway.rs` | `b39e079256a3baa05d0adf33a80b2ee1d469f47231a94fe20c172b8801b9e9dc` |
| `crates/core/src/gateway_runtime.rs` | `8cdc6971ded68f072b03239b3d181fed200c89671d16bc00595a0c622229acc5` |
| `crates/core/src/api.rs` | `aa3289620fb1a24244c18c0e8cb84e1413f57bd7b9fad17210abd6aa40283a5b` |
| `crates/core/src/api_operations.rs` | `545feee8bd09a3d190c8722eb2c844d105cd0439d49b1e625c45b5c90201e909` |
| `crates/media-addon/src/host.rs` | `7c8794dc40d845391b7c264c73168be0838f3e16adc8933799912abce9792b95` |
| `crates/media-addon/src/session.rs` | `dd00ab29465b3d0f9c6e9b73459e149dbf3ab6103d8acfa750afcdeb77b0fea8` |
| `docs/desarrollo-conservador.md` | `a17857b348e377cd1aa5d0c0e776c80e78da07b5dbd6cf927a2352bc50a7f6ea` |
| `docs/plan-skills.md` | `4e13fd111374c2f12031bf798d9cbaec8e69d3c4146f3877db047a6e54919da9` |
| `version.json` | `6a31b9097c0720cfbffe6ae2bc2687669084bd1c9014dcc130035fbcccb6f6da` |

## 7. Cierre documental

Se contrastaron las 20 huellas del apartado 6 tras redactar el plan: ninguna cambió. Se revisó la cobertura de R1–R6, la aclaración del micrófono, las dependencias y la separación entre evidencia y comprobaciones propuestas. El único archivo creado por esta tarea es este plan; los cambios locales previos se conservaron. La comprobación de espacios de Git se aplicó también al archivo nuevo mediante comparación con un temporal vacío, sin añadirlo al índice: sin errores de espacios. No se realizó compilación, ejecución de pruebas ni reproducción del producto.

## 8. Ejecución posterior autorizada — 4 de noviembre de 2026

El usuario pidió ejecutar las skills y aplicar este plan. Se mantuvo la rama y la base, se revisó el contrato vigente y se implementó en la variante estándar. Tipo inferido **Mediana**, por los recorridos funcionales y pequeñas adiciones: versión de fuentes **0.11.0**, `release: null`. Se conservaron los cambios previos citados en el apartado 1. No se añadió una aprobación intermedia ni se extendió la autorización a cuentas, dispositivos, publicación o instalación.

| Tanda | Estado de ejecución |
|---|---|
| T1 | Código implementado y pruebas sintéticas. Descubrimiento acreditado en código público del cliente oficial: CALL_CONNECT, op13, canal visible. Salida local inmediata, cierre nativo observado, un dueño de señalización y confirmación/barrier. La continuación conserva el suelo de autoridad de consultas sin nonce incluso tras timeout/reconsulta y respuestas en ambos órdenes; 64 registros de autoridad, historia exacta de 4096 canales y máximo cuatro consultas sin respuesta por canal. DM y grupos permiten actualización explícita; una consulta opcional rechazada no bloquea el chat. Validación con el servicio real pendiente. |
| T2 | Diagnóstico, matriz de dobles y análisis ABI del motor actual. 39 tests predeterminados, 41 con feature diagnóstico; probe N-API con ocho combinaciones y 65 cierres correctos. FakeADM nativo conservó callbacks/paquetes al cambiar descriptor y crear/destruir OwnStream. El probe de vídeo ficticio falló sin frames. La causa del incidente físico y la audibilidad real siguen pendientes; **no se declara R2 solucionada**. |
| T3 | Presentación optimista separada de política confirmada, proyección compartida, una escritura y última elección, errores/Unknown/429 y comprobación explícita. Emisión Gateway fallida se repara sin repetir PATCH. |
| T4 | Cabecera ampliada en una fila con icono Miembros, Volver y Ajustar; adaptación horizontal y pruebas de egui. Verificación visual manual pendiente. |
| T5 | Cadena menú → history → regreso corregida, contexto correlacionado, permisos, caché vacía válida y demanda fresca al cancelar carga o expulsar una página. Regresiones de UI y controlador. |
| T6 | Revisión y comprobaciones de código realizadas dentro del alcance. El registro final de comandos y huellas está en la nota de implementación y su evidencia. Los casos reales de cuentas/audio permanecen explícitamente pendientes. |

Cambios materiales respecto del diseño inicial: no se inventó semilla de llamadas en READY; se acreditó y utilizó la consulta dirigida del cliente oficial. El análisis posterior del motor nativo confirmó actualización incremental de `streamParameters`: omitir audio en un delta de vídeo no elimina el audio existente. Se mantuvo ese contrato, sin una corrección especulativa. Se añadió propiedad del proceso durante startup/cancelación y cambio de cuenta para que cerrar visualmente no habilite una segunda captura prematura. La caché RAM distingue vacío legítimo de ausencia sin aumentar su límite de tres canales.

La [nota de implementación 0.11.0](llamadas-presencia-navegacion-0.11.0.md) describe las fuentes, decisiones, resultados y límites; la [nota del micrófono](microfono-0.11.0.md) mantiene el diagnóstico para continuar T2. El pedido posterior «Ejecuta todo con la skill de notdiscord-programar» continuó la misma versión y añadió entrega de producción, registrada en la [nota de entrega](entrega-local-0.11.0.md). Las 939 pruebas del workspace pasaron, además de las 41 del addon diagnóstico que se solapan con las 39 de media. Build, check de todos los destinos release, verificación del paquete y siete comprobaciones IPC correctos. La publicación autorizada posteriormente quedó verificada en GitHub como `NotDiscord 0.11.0`, con etiqueta `NotDiscord-v0.11.0` sobre las fuentes `9b0df68a2b0a78bd8fda420906713ffa6a7658b1`; nombre, estado, tamaño y digest del asset coinciden con Setup. El manifiesto se anuncia después del asset. No se realizaron pruebas con cuentas/dispositivos físicos ni se instaló Setup; los casos reales de R2 y del servicio quedan pendientes, sin declarar corregido el micrófono.

## 9. Continuación de planificación — 6 de noviembre de 2026

**Corte histórico de planificación: análisis terminado y T7–T13 pendientes en ese momento.** El pedido posterior de implementación continúa esta ronda en 0.12.0; su estado se registra en la nota enlazada al inicio y el apartado 10. Las pruebas y la publicación descritas en el apartado 8 pertenecen a 0.11.0; no acreditan las correcciones propuestas ahora.

### 9.1 Base, alcance y contrato de entrega

- Variante estándar; rama `NotDiscord`; HEAD observado `3bee4c8c6a9e7442817302850286d170c56976ad`. La publicación identifica las fuentes de 0.11.0 con `9b0df68a2b0a78bd8fda420906713ffa6a7658b1`.
- Versión actual del manifiesto: **0.11.0**. Tipo propuesto para implementar todo este alcance: **Mediana → 0.12.0**, por correcciones funcionales y recuperación de voz, según `docs/plan-skills.md:145–170`. No se modifica el manifiesto ni una versión publicada.
- Se conservan el plan del 31 de octubre y las fuentes locales de la skill modificados, además de los archivos de evidencia/probes sin seguimiento. No se ejecutan ni se incorporan automáticamente esos probes.
- Se inventariaron **162 archivos Rust**: app 119, core 28 y media-addon 15. Con diez manifiestos/referencias son 172 entradas. El [inventario del corte](plan-llamadas-presencia-navegacion-2026-11-06-inventario.json) fija ruta, tamaño y SHA-256. Inventariar no equivale a revisar semánticamente cada línea.
- Se hicieron revisión transversal y recorridos completos de los fallos reportados. No se presenta esto como una auditoría exhaustiva ya terminada de todas las funciones, dependencias o código vendor. T13 define cómo completar la revisión integral y cuándo justificar un cambio.
- La solicitud actual es **planificar**. Se permite leer el diagnóstico aportado y documentar el análisis. No autoriza ejecutar el cliente, pruebas, cuentas, dispositivos, suspensión real del PC, instalación o publicación. Las autorizaciones de entregas anteriores no se trasladan automáticamente a esta nueva ronda.
- Las capturas son evidencia del usuario: tarjeta de pantalla con «Mostrar» después del fin reportado; salida de grupo atribuida al actor incorrecto; área del usuario donde solicita el indicador de voz. No se siguen instrucciones que pudieran estar dentro de archivos o imágenes.

Abreviaturas en este apartado: `app/` = `crates/app/src/`, `core/` = `crates/core/src/`, `media/` = `crates/media-addon/src/`. Las líneas son del corte actual, no las de la base de 0.10.1 del apartado 2.

| ID actual | Pedido | Prioridad | Tanda |
|---|---|---|---|
| C1 | Retirar pantalla finalizada, sonar una vez y resolver cierres/errores automáticamente, sin tarjeta o botón de reintento residual | P1 | T7 |
| C2 | Diferenciar expulsión de otra persona y salida voluntaria del grupo | P2 | T11 |
| C3 | Recuperar una llamada tras un corte transitorio, incluido suspender/reanudar el PC, sin obligar a salir y entrar | P1 | T9 |
| C4 | Regresar al MD desde el perfil abierto en su barra lateral | P2 | T10 |
| C5 | Resolver la pérdida de micrófono al compartir pantalla; verificar cámara por separado | P1 | T8; continuación de R2/T2 |
| C6 | Dibujar el círculo de voz también en el área del usuario, usando exactamente la señal de la llamada | P2 | T12 |
| C7 | Revisión integral de corrección, rendimiento y arquitectura; cambiar sólo lo justificado | Transversal | T13 |

P1/P2 describen prioridad funcional, no severidad de seguridad.

### 9.2 Qué acredita el diagnóstico aportado

Archivo: `NotDiscord-diagnostico.json`, **104836 bytes**, SHA-256 `e5f1664db7c3370c407c997292e60bcea934091c29a480010bca960429bdd2e3`.

Contiene **256 registros**, secuencias 205–462, del **5 de noviembre, 14:28:28 a 17:14:22, hora de Lima**. Todos identifican versión 0.11.0 y build `5b6286f2043eab9772d2ddd68352bf044f4d4d3ee296f4647b52b684ffba766c`. Ese identificador coincide con el SHA-256 comprobado de `evidence/source-cut-0.11.0-delivery.json` y con `docs/entrega-local-0.11.0.md:21–29`: es un inventario de entradas de compilación, no un hash de commit.

El ring conserva 256 entradas y expulsa las primeras (`core/diagnostics.rs:193–205`). `dropped_before=0` no demuestra que la exportación contenga toda la sesión ni todos los eventos de red. Los enteros de correlación se descodificaron sin pasarlos por números JavaScript de precisión insuficiente.

| Secuencia / hora de Lima | Evidencia | Interpretación limitada |
|---|---|---|
| 412–413 / 16:30:06–09 | Controller TransportClosed y Recoverable, correlación 1 | Hubo cierre y recuperación del Gateway principal. No acredita por sí solo recuperación de voz |
| 421 / 16:54:12.676 | Voice TransportIo, fase Read, sesión efímera 2; luego cierre de sesión de vídeo y retirada de captura | Compatible con pérdida de la sesión de pantalla; no demuestra la causa de red ni una parada voluntaria |
| 430–433 / 16:54:16.868 en adelante | Voice TransportIo Read, sesión 1; voz/cifrado pasan a falso y desaparecen participantes locales | Acredita una interrupción de voz en ese episodio; no identifica suspensión |
| 437–443 / 16:54:24 | Nuevo epoch MLS, transporte y luego cifrado listos | Vuelve a establecerse voz; el archivo no distingue si la entrada fue manual o automática |
| 451–455 / 16:54:32–33 | Nueva selección/inicio de pantalla; callbacks del micrófono pasan de 75 a 89, edad final 11 ms y cifrado listo | La captura entregó muestras en esas fases; no prueba emisión de paquetes ni audibilidad remota |

Hay 19 registros VoiceInputState. En las fases de pantalla de 14:28:28 y 15:25:05–06 también hay muestras recientes, elegibilidad local y cifrado listo. La puerta nativa figura cerrada en esas muestras de fase: sin el nivel/VAD deseado y los contadores de salida del mismo intervalo, puede ser silencio normal o parte del fallo. **No es prueba de un micrófono permanentemente cerrado.** Las muestras ausentes tras iniciar un epoch se distinguen de una edad real de 65535 ms mediante el bit de edad conocida.

La exportación no incluye una serie de estadísticas de paquetes de audio, eventos de energía del PC, los payloads de fin de stream ni una confirmación del receptor. La severidad warning de VoiceInputState es la clasificación genérica actual del grabador (`core/diagnostics.rs:197`), no una anomalía demostrada en cada muestra. Tampoco SoundQueued distingue qué sonido sonó. No se puede asignar el caso exacto de la captura a un registro concreto.

**Conclusión:** los cortes de transporte están acreditados; la hipótesis de suspensión y la causa del micrófono permanecen abiertas. La evidencia actual permite dirigir el diagnóstico sin repetir la hipótesis ya descartada de los parámetros de vídeo.

### 9.3 Hallazgos de fuentes y decisiones

#### C1 — Fuente remota, visualización y error todavía se mezclan en el cierre

- `app/controller/voice.rs::voice_dispatch:2920–2948` interpreta STREAM_DELETE como cierre de la suscripción observada, preservando la oferta del emisor salvo actualización de VOICE_STATE. La protección tiene un motivo real: un acuse de dejar de mirar no demuestra que el emisor dejó de compartir.
- `finish_watch:3196–3209` limpia recursos, pero conserva la intención con «La visualización terminó. Puedes volver a abrirla…». `fail_watch:3175–3194` puede reinsertar un descriptor con estado error.
- `app/controller/incoming.rs::incoming_media_dispatch:1916–1947` incorpora ofertas positivas desde STREAM_CREATE/UPDATE; la retirada depende de VOICE_STATE/membresía. Si no llega o no se acepta ese delta, queda `screen_sharing=true`.
- `app/ui/voice.rs:1370–1389,1493–1505` combina esa oferta y el error retenido para presentar Mostrar/Reintentar. Coincide estructuralmente con la tarjeta reportada; no reproduce la secuencia remota.
- `app/ui/sound_settings.rs::observe_sound_snapshot:264–278` produce StreamEnded al comparar `screen_sharing` de un participante existente en dos snapshots. Si la oferta permanece verdadera, no detecta el fin. Además, retirar al participante entero recorre otra rama; no debe duplicarse el sonido de salida.
- En el emisor propio, `media_event/shareState:3539–3565` llama a stop_share en estados terminales sin conservar/presentar su reason. Puede ocultar la causa de una parada inesperada. `stop_share:3233–3246` también espera el envío Gateway dentro del actor y descarta su resultado antes de refrescar.

**Confirmado:** esos comportamientos de estado, presentación, sonido y espera. **No confirmado:** qué evento concreto faltó en la sesión del usuario.

**Decisión:** una transición compartida que diferencie fin de publicación, cierre de visualización y fallo recuperable. Fin confirmado implica limpiar tarjeta, intención, ampliación y recursos y emitir un solo evento semántico de fin. Un fallo de receptor no permite inventar que el emisor terminó. El caso afectado se resuelve automáticamente dentro de un presupuesto acotado y no deja un botón Mostrar/Reintentar residual. Se conserva el control normal para elegir ver una transmisión que sí sigue activa.

#### C2 — El texto de expulsión usa siempre la salida voluntaria

`app/ui/chat.rs::system_record_description:3269–3284` devuelve siempre «salió del grupo» para tipo 2. `draw_call_record:1193–1209` lo antepone con el autor del mensaje, por lo que una expulsión aparece atribuida como salida del expulsor. El parser ya retiene autor y menciones (`core/parsing.rs:948–965`).

La prueba `ui/chat.rs:5094–5104` comprueba el texto devuelto por el propio helper, sin contrastar actor y persona retirada: no detecta esta semántica equivocada. La documentación oficial confirma que tipo 2 es RECIPIENT_REMOVE; no prescribe que toda retirada sea una salida voluntaria. [Discord: Message Resource](https://docs.discord.com/developers/resources/message#message-object-message-types).

**Decisión:** interpretar actor y destinatario del evento, validar la correspondencia de IDs y renderizar expulsión o salida según el caso. Si falta evidencia del destinatario, usar una descripción neutral; no adivinarlo por nombres ni hacer consultas de perfiles para pintar el registro.

#### C3 — Un fallo de voz cierra la llamada sin una ruta de recuperación

`media/host/voice_socket.rs::run/reader:118–185` termina el transporte ante IO/cierre/heartbeat. `media/host.rs::dispatch:506–510` llama a fail; `fail/close:1719–1750` emite error y desmonta recursos. El controlador `app/controller/voice.rs:3525–3528` llama a stop_voice y propone volver a unirse.

La apertura de medios envía Identify (`media/host.rs:504`); no se identificó una ruta de Resume de voz en ese recorrido. La recuperación del Gateway principal no restaura por sí misma esa sesión. La búsqueda transversal tampoco encontró tratamiento de WM_POWERBROADCAST/PBT_APMSUSPEND/PBT_APMRESUMEAUTOMATIC en las fuentes revisadas.

`socket_closed:2752–2769` convierte el código numérico en texto y un código genérico. Para decidir recuperación hace falta conservarlo como dato tipado, sin analizar frases traducidas. El diagnóstico aportado muestra IO, no un cierre 4014 ni prueba de expulsión.

Discord documenta Resume de voz con op7 y confirmación op9; en v8 incluye seq_ack. La posibilidad de reanudar depende de la sesión y de los mensajes retenidos. [Discord: Resuming Voice Connection](https://docs.discord.com/developers/topics/voice-connections#resuming-voice-connection).

4014, 4021 y 4022 no deben activar reconexión automática; 4015 permite intentar Resume. Errores de autorización/protocolo/cifrado requieren su tratamiento específico. [Discord: Voice Close Event Codes](https://docs.discord.com/developers/topics/opcodes-and-status-codes#voice-voice-close-event-codes).

**Decisión:** recuperación coordinada, máximo un intento automático por episodio, que conserve la intención de llamada. Distinguir interrupción recuperable, fin real y salida solicitada antes de destruir el contexto. No prometer mantener una conexión físicamente activa mientras el PC duerme.

#### C4 — El regreso del perfil usa el snapshot temporal de bienvenida

`app/ui/mod.rs:1733–1747` extrae el snapshot activo y deja `self.state=AppSnapshot::welcome()` durante el dibujo; lo restaura en 1809. `app/ui/shell.rs::draw_profile_inner:2765–2766` llama a `draw_chat_return_header`, cuyo wrapper (`app/ui/chat.rs:586–588`) utiliza `self.state.clone()`.

El botón envía así cuenta, generación, vista y canal de bienvenida (`chat_navigation_action:2881–2886`). `app/controller/observed_calls.rs::current_navigation_action:557–562` los compara con los activos y descarta la acción. El renderer del chat ya pasa el snapshot recibido; el perfil no.

La entrada desde el MD está en `draw_dm_sidebar_inner:2747–2748`; la entrada desde una fila de miembros en `shell.rs:2392–2399`. Ambas desembocan en el mismo perfil y retorno.

**Certeza:** fallo alcanzable y causa estática concreta. **Decisión:** exigir el contexto de dibujo recibido en el helper de retorno; retirar el wrapper inseguro. No arreglarlo eliminando las validaciones del controlador. De paso se elimina una clonación innecesaria de AppSnapshot, sin afirmar un ahorro de RAM medido.

#### C5 — El micrófono continúa sin una causa física acreditada

`media/session.rs::input_diagnostics:255–269` declara expresamente que puerta/callback no prueban entrega. `media/host.rs::Tag::MicLevel:1833–1867` separa detección, puerta y transmisión elegible; `app/controller/voice.rs:3634–3637` lleva speaking a la tarjeta. Un círculo verde puede reflejar actividad local sin que haya audio audible en el receptor.

El nuevo diagnóstico prueba muestras de entrada recientes al iniciar pantalla; no contiene paquetes salientes ni recepción remota del mismo intervalo. La nota `docs/microfono-0.11.0.md` y el apartado 8 ya distinguen los ensayos sintéticos de un micrófono físico. El análisis ABI posterior acreditó deltas incrementales: **no volver a cambiar streamParameters como si omitir audio lo eliminara**.

**Decisión:** continuar T2 con evidencia de detección deseada → puerta → cifrado → contadores por sesión → recepción. No cerrar la incidencia con más pruebas de fakes que sólo comprueben que las órdenes devuelven éxito. Cámara permanece en la matriz, sin declararla afectada de forma confirmada.

#### C6 — El área del usuario no consume el speaking de la llamada

`app/ui/voice.rs:1453–1464` dibuja el círculo desde `CallParticipant.speaking` y las guardas de mute/deaf. `app/ui/shell.rs:2050–2052` dibuja el avatar propio con la presencia, sin ese círculo.

**Decisión:** una señal/predicado compartido para el participante propio del intento de llamada actual. Área de usuario y tarjeta deben consumir el mismo booleano y las mismas guardas, sin un segundo detector VAD, timer o consulta. El círculo describe actividad de voz; no se redefine como confirmación remota.

### 9.4 Instrucciones conservadoras aplicables a esta continuación

Contrato leído: `docs/desarrollo-conservador.md`, consolidación del 4/11/2026 sobre `865b2f11…`; su huella está en el inventario. Este apartado transmite las reglas relevantes al implementador:

1. Toda tarea pertenece a cuenta/generación; añadir intento de voz, canal, publicación/observación y token de operación. El cierre o resultado tardío de una fuente no puede afectar a su reemplazo. Los eventos de UI llevan el contexto real recibido en el frame.
2. Un único dueño coordina recuperación de Gateway, voz y reintentos. Máximo un intento automático por episodio; coalescer cambios de red y los dos avisos de reanudación de Windows. No dar presupuestos independientes al host principal, receptor y pantalla que formen una ráfaga.
3. Admitir trabajo opcional sólo sincronizado y con permisos/contexto vigentes. La recuperación espera la sincronización necesaria, el fin de tareas anteriores y la liberación local real. Revalidar tras semáforos/rate limits y antes de emitir. No basta cambiar un flag de conectado.
4. Respetar Retry-After, Identify y cooldowns mayores. Los 30 s/60 s existentes son políticas del controlador que deben verificarse, no límites oficiales nuevos. Elegir el modo de recuperación antes de emitir: Resume si conserva una sesión válida; un único handshake fresco sólo si procede. Un Resume fallido no autoriza una cadena ilimitada de alternativas.
5. Hello, Resume/Identify, confirmaciones, heartbeat y DAVE son pasos obligatorios del intento. No simular conexión o transmitir con cifrado antiguo para evitar una espera. No quitar los watchdogs ni impedir la suspensión del equipo como solución.
6. Timeout/cancelación no acreditan fin de trabajo ni rechazo remoto. Retener resultado desconocido y posibles confirmaciones de mutaciones emitidas. No reenviar mensajes/subidas ni borrar Operations.Unknown, nonces o RateLimits para recuperar la llamada.
7. Una escritura/cierre en curso por intención. Las órdenes obligatorias de liberación conservan orden y completion, fuera del actor cuando esperan red; no usar spawn sin dueño ni liberar la barrera por un timeout de la UI. La salida del usuario y la pérdida de permisos revocan cualquier recuperación pendiente.
8. Fin del emisor y cierre del espectador son hechos distintos. Aceptar un false explícito, una retirada completa vigente o un fin de publicación acreditado; un campo omitido sigue siendo desconocido. Conservar guardas de membresía, tombstones y autoridad de respuestas sin nonce.
9. Una comprobación automática adicional sólo si el estado es ambiguo y hace falta resolver la intención vigente: una demanda coalescida, una emisión dentro del presupuesto compartido y sin polling. Usar get_once cuando sea HTTP; Api::request puede repetir ciertos GET ante 429. No llamar a la ruta explicit=true fingiendo un clic para saltarse el historial de consultas automáticas de observed_calls.
10. Conservar demanda visible válida y crear trabajo fresco tras sincronización. READY_CATALOG puede preceder a Synchronized: retener demanda antes de clonar Api o marcar trabajo activo. Invalidar tickets antiguos y backlog obsoleto; conservar confirmaciones ya iniciadas.
11. Reutilizar captura y señal existentes; no activar nuevos dispositivos, compartir una fuente distinta ni abrir micrófono en Go Live. Tras recuperación, sólo rearmar selecciones explícitas aún vigentes con permisos/cifrado listos. No recuperar una llamada terminada, una expulsión ni una cuenta anterior.
12. La limpieza visual y el círculo no requieren red. Los sonidos respetan configuración/DND y se emiten una sola vez por transición, independientemente de snapshots duplicados. No cambiar la política confirmada de notificaciones mediante una proyección visual.
13. Mantener límites de streams, bytes, colas, imágenes, texto y temporales. Diagnóstico local tipado y acotado: fases, razones fijas y contadores; sin grabaciones, credenciales, mensajes, pantallas, dispositivos o IDs privados. Conservar borradores, cachés válidas, ajustes y datos ajenos.

### 9.5 Tandas y aceptación

Orden recomendado: **T10 y T11** como correcciones pequeñas independientes; después **T7 → T8 → T9 → T12**; **T13** integra y cierra la revisión. El análisis de T8 puede preceder a T7; sus resultados deben contrastarse también con el ciclo de fuentes corregido. Trabajo secuencial, sin delegación requerida.

#### T7 — Fin de pantalla, sonido y errores automáticos (C1)

**Archivos:** controller/voice, incoming y observed_calls; modelos de fuentes si corresponde; ui/voice y sound_settings; eventos tipados del host.

- Introducir un estado de fuente/publicación distinto del estado del decoder o suscripción. Separar activo, recuperación acotada, finalizado e información insuficiente. Si se necesita identidad de publicación, usar cuenta/canal/owner y la revisión de publicación observada; no suponer que stream_key nunca se reutiliza.
- Centralizar la transición terminal. Fin confirmado retira oferta efectiva, descriptores, sinks, selección/ampliación e intención de pantalla; conserva cámara, participante y audio. Un callback tardío no resucita la publicación terminada.
- Clasificar STREAM_DELETE y estados del host: acuse propio de stopWatch, fin acreditado de publicación, stream_full/unauthorized y error transitorio. Verificar la semántica de razones no documentadas antes de asignarlas; no tratar todo DELETE como fin del emisor.
- Si sólo termina el receptor, resolver automáticamente la intención: comprobar una vez la fuente si es necesario y hay presupuesto, o recuperar una vez la recepción ya elegida si sigue vigente. No empezar a mirar a otras personas automáticamente. Fin confirmado limpia sin consultas extra. Un estado desconocido no dispara un bucle hasta conseguir una respuesta conveniente.
- Retirar la tarjeta o botón Mostrar/Reintentar del caso terminal reportado. Un agotamiento de recuperación se expresa mediante un aviso discreto y causa conservada, sin dejar un control inoperante. La tarjeta normal para elegir ver una fuente realmente activa se mantiene.
- Emitir StreamEnded desde una transición semántica deduplicada por publicación, evitando que la misma transición vuelva a sonar por comparar snapshots. Preservar preferencias y la política existente para salida de participante/llamada; no producir dos sonidos por el mismo fin.
- Conservar razón tipada de las paradas inesperadas propias antes de stop_share. Mostrar la causa saneada y fase; no sustituir todo error por una invitación a reintentar. Llevar stopShare/StreamDelete a un cierre con propietario/completion fuera del actor y publicar inmediatamente el cambio local.

**Aceptación propuesta:** stop normal remoto → desaparece sólo pantalla y suena una vez; stopWatch local → no retira la oferta ajena ni suena como fin del emisor; false explícito antes/después de DELETE; publicación nueva con la misma key y ACK viejo; fin sin estar mirando; desaparición del participante; cámara simultánea; error nativo/IO, pantalla llena o no autorizada; timeout de cierre con tarea aún viva. Confirmar automáticamente el caso afectado sin botón residual, sin tráfico por frame y sin cierre de voz como arreglo.

#### T8 — Localizar y corregir la pérdida de micrófono (C5; R2 reabierto)

**Archivos:** media/session, host, stats; adaptador app/diagnostics y controller/voice; fixtures existentes.

- Partir del build identificado y del contrato ABI ya acreditado. Conservar los resultados anteriores como contraevidencia de la hipótesis de sustitución total de parámetros.
- Añadir a un resumen local acotado los datos que faltan: detección VAD/PTT deseada, puerta aplicada, cifrado, edad/cantidad de callbacks y delta de contadores de audio de la sesión principal. Separar audio de Go Live del micrófono y generación de contadores.
- Capturar alrededor de selección, inicio, fallo y parada de vídeo, y cuando exista voz deseada sin progreso saliente durante una ventana justificada. Propuesta inicial para una captura diagnóstica voluntaria: hasta 10 s, como máximo una muestra de stats por segundo y finalización/cancelación explícita. Es un presupuesto propuesto de observación local, no un nuevo polling permanente ni tráfico HTTP.
- Discriminar: entrada detenida; VAD/PTT sin abrir; puerta/cifrado revocados; audio sin codificar/salir; paquetes que salen pero no se reciben/descifran. Contadores genéricos de transporte con vídeo no acreditan paquetes de micrófono.
- Corregir el tramo demostrado. Un cambio global de dispositivo/callback tiene que tener dueño; una sesión de vídeo no lo retira de la voz principal. Un rekey sólo reabre transmisión al confirmar el estado seguro. No reconectar la llamada completa o forzar mute/unmute periódicamente como parche.
- Conservar sin cambios de significado la señal de actividad visual mientras se arregla el transporte; impedir que una señal vieja quede encendida tras cierre/reemplazo.

**Aceptación propuesta:** voz base y conversación bidireccional; iniciar/parar/cambiar pantalla con sonido de aplicación sí/no; cámara sola y combinada en ambos órdenes; VAD/PTT, mute/deaf, ganancia cero/restaurada y rekey. Dobles verifican propiedad y secuencias; un ensayo autorizado con receptor verifica audibilidad sostenida. Si sólo hay muestras locales o fakes, registrar la mejora/instrumentación y mantener la causa física pendiente: no declarar C5 resuelto.

#### T9 — Recuperación de voz y suspensión del PC (C3)

**Archivos:** adaptador de energía en app/platform o módulo dedicado, coordinador de conexión/voz, protocolo de medios y voice_socket. No poner lógica de recuperación en el dibujo.

- Añadir eventos tipados de suspensión/reanudación y procesarlos fuera de WndProc; sin bloquear ese callback con red. Windows diferencia aviso de reanudación automática y actividad de usuario posterior: coalescerlos por episodio. [Microsoft: PBT_APMRESUMEAUTOMATIC](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumeautomatic).
- Conservar intención de llamada y estado de controles por cuenta/intento; al suspender cerrar transmisión y descartar timers/frames obsoletos sin enviarlos en lote al despertar. Un salto de reloj o un periodo sin frames, por sí solo, no prueba suspensión; registrar el origen del evento.
- Conservar tipo de transporte y código de cierre numérico en MediaError/IPC. Definir tabla pura de recuperación: IO/heartbeat/4015 pueden ser candidatos; autorización, protocolo, expulsión, fin de llamada y límites terminales detienen la recuperación.
- Separar Recovering de salida definitiva. Esperar Gateway y contexto válidos, seleccionar un modo y ejecutar un único intento. Resume v8 conserva seq_ack y espera op9; si el contexto anterior ya no es utilizable, preparar un único handshake fresco permitido, con credenciales de voz nuevas en RAM. No persistir tokens de voz ni extraerlos de una cuenta ajena.
- Invalidar el socket viejo y observar fin/cancelación antes de reemplazarlo; conservar o reconstruir transporte nativo/DAVE según la validez real. No abrir dos capturas. Las confirmaciones retrasadas o mensajes DAVE repetidos no reactivan un epoch viejo.
- No enviar VoiceState(channel=null) de salida voluntaria sólo por el fallo transitorio que se intenta recuperar. La salida real del usuario sí cancela todo; cambio de canal/cuenta/permisos también invalida la intención.
- Audio/selecciones existentes sólo se rearman cuando el nuevo estado y cifrado se confirman. Si falla el intento, detenerse y conservar una explicación real. No prometer recuperación si el servidor finalizó la llamada.

**Aceptación propuesta:** fixture de sleep/wake; avisos de reanudación duplicados; caída de IO sin suspensión; heartbeat sin ACK; Gateway aún recuperándose; op9 correcto; sesión inválida; 4014/4021/4022; salir/cambiar cuenta durante espera; timers vencidos, socket viejo bloqueado y callbacks tardíos. La caída transitoria recuperable no exige volver a entrar manualmente y usa como máximo el presupuesto acordado. Una prueba real de suspensión se realiza sólo dentro de un pedido que autorice ese efecto en el PC.

#### T10 — Retorno de perfil con el contexto real (C4)

**Archivos:** ui/chat, shell; validadores de navegación y sus pruebas.

- Un único helper exige `&AppSnapshot` del frame. Pasarlo desde draw_profile_inner y revisar todos los callers de retorno. Retirar el wrapper que consulta/clona self.state durante el dibujo.
- Mantener cuenta, generación, canal/vista de origen y validación de resultados tardíos. No quitar current_navigation_action ni el chequeo de permisos para conseguir que funcione.
- Conservar borrador, scroll y caché; si el chat ya no está disponible, ofrecer una salida coherente con el estado vigente.

**Aceptación propuesta:** recorrer el frame real de la aplicación, donde self.state está temporalmente en welcome: MD → barra lateral → perfil completo → volver al MD. Incluir clic desde fila de miembros, retorno a history/pins cuando corresponda, sin conexión, cambio de cuenta/canal y doble clic. La prueba no debe prellenar artificialmente self.state de modo que esconda el defecto.

#### T11 — Actor y persona retirada en registros de grupo (C2)

**Archivos:** ui/chat y helper compartido del registro; parser/modelo sólo si la evidencia muestra que falta un dato.

- Fixture tipo 2 con autor A y destinatario B → «A eliminó a B del grupo».
- Fixture de salida voluntaria de B → «B salió del grupo», según identidad acreditada.
- Destinatario ausente/incompleto → texto neutral, sin inventar expulsor o persona expulsada.
- Reutilizar datos de author/mentions disponibles y nombres visibles; comparar IDs, no texto. Mantener selección/copiar/enlaces y la misma semántica en chat, búsquedas, pins y registro RAM.

**Aceptación propuesta:** nombres iguales con IDs distintos, nombres cambiados, múltiples menciones inesperadas, payload parcial y mensaje histórico. Aserciones de texto esperado independientes del helper, no el resultado del mismo helper usado como oráculo. No hacer red para renderizar.

#### T12 — Círculo propio compartido (C6)

**Archivos:** ui/voice, shell y helper de presentación de voz.

- Factorizar el predicado ya utilizado por la llamada: participante propio del intento activo, speaking y guardas de mute/deaf/servidor. Consumirlo en ambas superficies usando el snapshot del frame.
- Dibujar el mismo círculo alrededor del avatar del área de usuario, conservando el punto de presencia, menú y controles de la captura.
- La señal se apaga en silencio, salida, pérdida de sesión/cifrado o reemplazo según el contrato común. No iniciar captura, detector o temporizador de audio adicional.

**Aceptación propuesta:** dos círculos coinciden en el mismo snapshot, VAD/PTT, controles de silencio, navegar a otra conversación y minimizar/restaurar. Sin llamada no hay círculo de voz. La visibilidad del panel de llamada no gobierna la señal del área de usuario.

#### T13 — Revisión integral con cambios condicionados (C7)

**Objetivo:** completar la revisión del inventario por responsabilidades y cerrar las regresiones; optimizar sólo donde se demuestre necesidad. No es una autorización para una reescritura general.

1. Recorrer entradas/salidas y propietarios de cada área de la matriz siguiente. Registrar por archivo: revisado, hallazgo, sin cambio justificado o pendiente, con evidencia. Separar tests/probes y vendor; no evaluar arquitectura por tamaño de archivo contando pruebas.
2. Para cada refactor exigir un fallo alcanzable, trabajo duplicado significativo o una frontera de propiedad que impida corregir/verificar el caso. Las extracciones de ciclo de fuente, recuperación y contexto de navegación están justificadas por C1/C3/C4; no migrar todo UiAction/JSON a otro framework por uniformidad.
3. Para cada optimización exigir escenario comparable, métrica, referencia antes/después y límite de regresión. Medir tiempo del actor/UI, p95 de frame, contadores de trabajo, colas, working set/comprometida y proceso de medios por separado. Ninguno se midió en esta planificación; no prometer ahorro de CPU/RAM.
4. Revisar errores ignorados, esperas dentro del actor, I/O y locks de UI, cancelación y autoridad de snapshots. Priorizar stop_share, errores terminales y el contexto falso ya identificados; un unwrap o una cola por sí solos no son un fallo.
5. Conservar límites/cachés y eliminar trabajo repetido antes de aumentar capacidad. Revisar dependencias según Cargo.lock/target/features; no actualizar paquetes ni alegar CVE por antigüedad. Una auditoría de avisos vigente requerirá fuentes primarias y alcance explícito.
6. En una continuación de programación autorizada, usar comprobaciones proporcionales: formato de lo cambiado, check de todos los destinos, tests de la cadena UI→actor y de transiciones de medios/recuperación. Clippy o formato global no convierten cambios masivos en necesarios. No reejecutar suites que ya pasen sin cambios o dudas que lo justifiquen.
7. Cerrar cada requisito con resultado y limitaciones; el micrófono exige evidencia de entrega real si se afirma solución audible. Mantener pendientes honestos si sólo se pudo instrumentar.

### 9.6 Matriz transversal y criterio de intervención

Esta matriz registra lo observado en la revisión de planificación y dirige T13. «Conservar» no certifica ausencia de todos los fallos.

| Área | Evidencia revisada / controles existentes | Decisión actual |
|---|---|---|
| Estado/UI/navegación | Extracción temporal del snapshot, guardas de cuenta/generación; SharedList copy-on-write | Corregir la lectura de welcome y retirar clonación innecesaria; revisar callers. No reemplazar egui ni el modelo de snapshots |
| Llamadas/fuentes/sonidos | Anuncio, intención y decoder repartidos; cierres con error retenido; sonidos por diferencia de snapshots | Centralizar transición terminal y su evento deduplicado; necesaria por C1 |
| Audio/IPC/transporte | Puertas VAD/PTT/DAVE, sockets y colas acotados, diagnóstico saneado | Diagnóstico dirigido y recuperación con dueño; no relajar cifrado, mute ni límites |
| Red y envíos | api.rs:211–237,463–523; outgoing.rs:1–61; Operations/RateLimits y nonce | Preservar tickets, confirmación e incertidumbre. Revisar integración de recuperación; sin cambio general de política |
| Catálogos/presencia/miembros | observed_calls acotado; demanda visible; intención visual separada de política | Reutilizar y coordinar comprobación de fuente; no consultas por repintado ni backlog |
| Texto/registros/Markdown | Tipo 2 ignora destinatario; markdown_cache con límites; presupuestos de borradores | Corregir semántica del registro. Mantener cachés/parser salvo medición o defecto adicional |
| Imágenes/vídeo/memoria | resources: colas limitadas; ui/video.rs:5–29 usa último frame y reutiliza textura; core/video limita streams/bytes; MessageCache LRU acotada | No hay medición que justifique cambiar cuotas o prometer ahorro. Medir sólo un escenario relevante y preservar liberación/leases |
| Adjuntos/subidas | Validación local de archivos/lotes y refresh dirigido get_once | Conservar borradores, permisos y límites. Revalidar límites del servicio si una implementación los modifica |
| Autenticación/datos/RPC | storage DPAPI y escritura atómica; auth_job posee procesos; shared_state conserva campos; RPC verifica usuario/sesión | Mantener aislamiento y cancelación; ningún cambio general justificado por estos incidentes. No se declara auditoría de seguridad completa |
| Actualizador/distribución | Staging con propietario, lease, allowlist y cuotas; manifiesto/asset verificados en entrega anterior | Mantener flujo y separación de variantes; publicar sólo si se solicita |
| Pruebas/diagnóstico | Pruebas de registro comparan contra su propio helper; exportación de 256 entradas no contiene contadores de emisión | Añadir casos de actor/destinatario y frame real; mejorar evidencia útil, no cantidad de logs |

### 9.7 Comprobaciones, recuperación y estado de entrega

**Realizado aquí:** lectura y descodificación del JSON aportado; contraste de su build con el inventario de 0.11.0; revisión de código y documentos; inventario de los tres crates; consulta de documentación primaria de Discord/Microsoft; actualización de este plan y anexo. Las capturas y observaciones no se presentan como reproducciones del agente.

**Pendiente de implementación:** T7–T13. No se ejecutaron compilación, tests, probes, cliente, dispositivos ni suspensión del equipo en esta ronda. La revisión exhaustiva por archivo y las mediciones de rendimiento son trabajo previsto en T13, no resultados existentes.

Al implementar, conservar datos y configuración sin migraciones nuevas. Intenciones/episodios de recuperación son efímeros y pertenecen a una cuenta. Para revertir, identificar el cambio concreto y preservar los cambios ajenos; no restaurar una sesión ni borrar carpetas para ocultar errores. Un rollback visual no revierte operaciones ya aceptadas por el servicio.

| Tanda | Estado al entregar esta planificación | Evidencia exigida para su cierre |
|---|---|---|
| T7 | Pendiente | Fin real/stopWatch/error diferenciados; sin tarjeta residual, sonido único y recuperación acotada |
| T8 | Pendiente; continúa T2 | Tramo de pérdida localizado y prueba pertinente; audibilidad remota si se afirma C5 resuelto |
| T9 | Pendiente | Recuperación y códigos terminales correctos; episodios coalescidos y cancelación segura |
| T10 | Pendiente | Retorno desde el frame real, sin eliminar guardas ni perder origen |
| T11 | Pendiente | Actor/destinatario y salida voluntaria diferenciados con expectativas independientes |
| T12 | Pendiente | Misma señal y guardas en ambos círculos |
| T13 | Inventario y revisión transversal realizados; revisión integral/mediciones pendientes | Cobertura por responsabilidad, sólo cambios fundamentados y resultados comprobados |

No hay aclaración del usuario necesaria para entregar el plan. Si la implementación necesita un ensayo de cuenta/dispositivos o suspensión real que todavía no esté autorizado, primero debe preparar código, instrumentación y un procedimiento concreto y limitado; la solicitud de ensayo no sustituye el trabajo independiente. No se crea una aprobación artificial del plan.

**Cierre del corte documental:** 172 huellas contrastadas sin cambios; el diagnóstico original conserva su SHA-256. Anexo JSON válido con 172 entradas, ambos documentos sin espacios finales y diff del plan sin errores de formato. HEAD se mantuvo en `3bee4c8c6a9e7442817302850286d170c56976ad`. Esta ronda sólo actualiza el plan existente y añade su inventario; conserva las modificaciones previas y no modifica código, manifiestos ni datos del producto.

## 10. Ejecución de 0.12.0 — 6 de noviembre de 2026

Los pedidos posteriores de ejecución y continuación autorizaron implementar las
tandas con `notdiscord-programar`. Se mantuvieron variante estándar, rama y base,
y se adoptó **Mediana → 0.12.0** por las correcciones funcionales y recuperación.
`version.json` contiene esa versión con `release: null`. Los límites de cuentas,
dispositivos, suspensión real e instalación permanecieron vigentes. Esta ronda
prepara entrega local; la publicación verificada de 0.11.0 no anuncia 0.12.0.

| Tanda | Estado de implementación y comprobación |
|---|---|
| T7 | Implementada; fixtures de autoridad de publicación, fin único, stopWatch, sustitución, fallos, cierre con dueño/ACK y conservación de voz/cámara correctas |
| T8 | Instrumentación voluntaria acotada implementada y comprobada con fakes. **C5 abierto:** causa física y audibilidad remota sin acreditar |
| T9 | Implementada; presupuesto compuesto, Resume/Fresh, códigos terminales, generaciones, join y energía coalescida comprobados con fixtures y loopback; servicio y suspensión real sin probar |
| T10 | Implementada y fixture del snapshot extraído del frame correcta |
| T11 | Implementada; expectativas independientes para actor/destinatario, metadatos incompletos y copia correctas |
| T12 | Implementada; predicado compartido y fixture de ambos círculos correctos; sin ensayo físico de voz |
| T13 | Revisión estática por responsabilidades de **174/174** archivos Rust de `src`; sin faltantes. Sólo correcciones justificadas; sin métricas CPU/RAM/p95 ni auditoría de dependencias/vendor |

Cambios materiales de integración: Fresh libera todos los propietarios locales
antes de esperar Gateway/Sends; el diagnóstico distingue hosts principales
sucesivos aunque repitan token de captura; la parada propia conserva causa
clasificada; el lease de preview sobrevive a timeout/cambio de selección. Se
corrigió el E0425 de `preview.rs` y las dos rutas de preview se ejercitaron con
PNG/RGBA sintético. Ninguno de estos cambios demuestra audibilidad remota.

Pasaron **995 pruebas** de librerías del workspace (766 app, 176 core, 53 media),
con cinco ignoradas existentes, y las **55** del addon diagnóstico que se solapan
con media. Cinco fallos iniciales de acceso denegado a pipes/junctions desaparecieron
al ejecutar las mismas fixtures en el contexto de Windows que necesitan. Los
dos drivers N-API usan objetos falsos: ocho ciclos de publicación y 64 de
receptor, callbacks tardíos, ambas API de preview y cierre de GoLive sin main;
sin motor de terceros, dispositivos ni conexiones de red en esos probes.

La [nota de implementación](llamadas-presencia-navegacion-0.12.0.md) y el
[inventario T13](../evidence/t13-review-0.12.0.json) detallan decisiones, evidencia
y límites. El corte de fuentes de producción se fija por SHA-256 de bytes de
trabajo; no se presenta como commit ni certificación de ausencia de errores.
La compilación de producción y el check release de todos los destinos pasaron;
el paquete de 41 archivos se verificó, pasaron siete comprobaciones IPC sin
inicialización nativa y se generó `NotDiscord-Setup-0.12.0.exe` de 119095983 bytes.
El [acta local](../evidence/delivery-0.12.0-local.json) fija hashes y límites.
Los metadatos pendientes coinciden con Setup y el ejecutable; `release` sigue
siendo null y no se verificó su URL futura. No se ejecutó Setup ni se hizo
commit, push o publicación. No se alteraron instalación, datos ni otras
variantes, ni se invocó una aprobación nueva para el trabajo ya autorizado.

## 11. Publicación posterior autorizada — 6 de noviembre de 2026

El usuario pidió publicar esta versión y ajustar `notdiscord-programar` para
publicar al terminar los planes de cualquier tipo de actualización. La skill del
repositorio y la instalada se actualizaron y validaron; no se amplió la autorización
a cuentas, dispositivos, suspensión real ni instalación.

Se publicó el commit de fuentes `9eb222ffd3a3e962106864317c6cb0a674f13209`, con
`release: null`. El mapa Git vincula los 184 inputs del binario con los blobs del
commit, distinguiendo la normalización de texto de los bytes de compilación; el
corte y las actas locales mantienen sus bytes históricos. La etiqueta
`NotDiscord-v0.12.0` apunta a ese commit. La release se titula exactamente
`NotDiscord 0.12.0`, sin estado draft ni prerelease. El asset de 119095983 bytes
tiene el SHA-256 `5097b8b7d034ffb5c5b7e8786c5984a0654c41f6b3bf855aa063f632f8874915`,
igual a Setup local; su enlace público respondió HTTP 200.

El manifiesto se anuncia después de verificar el asset. El
[acta de publicación](../evidence/publication-0.12.0.json) recoge las comprobaciones
y los límites; no altera el acta local previa ni afirma solucionado C5.
