# Plan de llamadas, micrófono, presencia y navegación — 4 de noviembre de 2026

Estado actual: **T1, T3, T4 y T5 implementadas; T2 instrumentada y cubierta con dobles, sin causa nativa confirmada; T6 revisada y verificada dentro del alcance autorizado**. El pedido posterior del usuario autorizó ejecutar este plan aplicando las skills de planificación y programación. La [nota de implementación](llamadas-presencia-navegacion-0.11.0.md) mantiene el resultado vigente.

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

La [nota de implementación 0.11.0](llamadas-presencia-navegacion-0.11.0.md) describe las fuentes, decisiones, resultados y límites; la [nota del micrófono](microfono-0.11.0.md) mantiene el diagnóstico para continuar T2. El pedido posterior «Ejecuta todo con la skill de notdiscord-programar» continúa la misma versión y añade entrega local de producción, registrada en la [nota de entrega](entrega-local-0.11.0.md). Las 939 pruebas del workspace pasaron, además de las 41 del addon diagnóstico que se solapan con las 39 de media. Build, check de todos los destinos release, verificación del paquete y siete comprobaciones IPC correctos. Setup 0.11.0 generado, con hashes y manifiesto borrador locales. No se realizaron pruebas con cuentas/dispositivos físicos ni se instaló Setup; los casos reales de R2 y del servicio quedan pendientes, sin declarar corregido el micrófono. La publicación autorizada posteriormente se registra en la nota de entrega.
