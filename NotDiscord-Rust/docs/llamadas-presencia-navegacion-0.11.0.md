# Llamadas, presencia y navegación — implementación 0.11.0

4 de noviembre de 2026. Variante estándar, rama `NotDiscord`, base `865b2f11069fadcd40f40bc634e7162ea7694f3e`. El usuario pidió ejecutar el plan del mismo día. Se aplicaron `notdiscord-planificar`, `notdiscord-programar` e investigación estática con `reverse-engineer-anything`. Los cambios previos de README, plan del 31 de octubre, skills, contrato conservador y fixtures ajenos se conservaron.

Tipo **Mediana**, inferido por los recorridos funcionales y pequeñas adiciones; versión de fuentes **0.10.1 → 0.11.0**. `version.json` queda con `release: null`; no se reutilizan hashes del instalador publicado de 0.10.1. No se modificaron ramas Plus/legacy, instalaciones, accesos directos, cuentas o configuración del usuario. No se hizo commit, push ni publicación. El pedido posterior «Ejecuta todo con la skill de notdiscord-programar» continúa esta misma versión local; la entrega y sus hashes se registran en la nota de entrega enlazada al cierre.

## Estado por requisito

| Tanda | Implementación y evidencia | Límite vigente |
|---|---|---|
| T1 / R1, R6 | Consulta dirigida y tipada de la conversación visible; una operación activa, demanda diferida hasta sincronizar, sin polling ni ring. Actividad compartida entre tarjeta e indicador, roster completo/parcial, identidad de mensaje y tombstones. Salida local publicada antes de red; cierre de proceso y señalización con dueño y completions independientes del intento nuevo. | Mecanismo acreditado por código público oficial y fixtures; no se utilizó una cuenta para observar la respuesta real del servicio. |
| T2 / R2 | Diagnóstico saneado y matriz con motor simulado; ABI del motor actual inspeccionada y continuidad de captura/paquetes acreditada con FakeADM nativo al cambiar descriptor de vídeo y crear/destruir OwnStream. | **El incidente del micrófono físico sigue sin causa probada ni corrección funcional acreditada.** El probe de vídeo ficticio agotó su espera sin frames; no se presenta como éxito. Véase `microfono-0.11.0.md`. |
| T3 / R3 | Intención visual por cuenta/generación/revisión, independiente del ajuste confirmado. Proyección compartida de avatar, miembros, presencias y perfil; caché de listas. Una escritura activa y última elección coalescida; rechazo, 429, no enviado, desconocido y PATCH persistido con emisión Gateway fallida diferenciados. | Flush de Gateway confirma la emisión, no lo que ve otro cliente. |
| T4 / R4 | Icono Miembros, Volver a participantes y Ajustar en una fila; nombre truncado y desplazamiento horizontal cuando no caben. Menú, Escape, zoom y pan conservados. | Tests de egui; revisión visual manual de todos los DPI y ventanas aún pendiente. |
| T5 / R5 | Menú emite `history`; regreso correlaciona cuenta/generación/canal/guild/vista y revalida acceso. Conserva selección, borrador, adjuntos y ancla existentes. Distingue página vacía válida de caché ausente/expulsada y conserva demanda ordinaria cuando se canceló una carga inicial. | Sin reproducción manual con una cuenta. El registro continúa sólo en RAM. |
| T6 | Revisión del flujo completo, tests sintéticos y comprobación de destinos. | Resultados finales de comandos al cierre de esta nota. |

## Descubrimiento acreditado

Se descargaron sin cookies ni credenciales el HTML público de [Discord](https://discord.com/app) y cuatro scripts referenciados por él. No se abrió un cliente ni se enviaron órdenes al servicio. Los originales se conservan en el temporal ignorado `.tools/temp/current/calls-0110-public-client/` del repositorio; el índice acotado está en `evidence/t1-call-discovery-source.json`.

La lectura de [web.0bcf9a9e83b2c3c7.js](https://discord.com/assets/web.0bcf9a9e83b2c3c7.js) acredita la cadena CallStore → acción `CALL_CONNECT` sólo con sesión establecida → `callConnect(channel)` → opcode **13**, cuerpo **`{"channel_id": channel}`**. No es un endpoint HTTP ni una conexión de voz. El mismo código procesa `CALL_CREATE` con `message_id`, `voice_states` y `ongoing_rings`; se conserva compatibilidad con el array histórico `ringing` sin transformar ausencia en vacío.

| Artefacto | Bytes | SHA-256 |
|---|---:|---|
| `discord-app.html` | 74250 | `3d88f1460b0689382161627251a42ef973c11130a44434489aba6647297e5671` |
| `fast-connect.e08a663f8e63dc24.js` | 33834 | `eca242f694fd00b8de10ff5bc8797f6b0ea940b5600cdc0b32d06defd3659518` |
| `web.0bcf9a9e83b2c3c7.js` | 12944614 | `e86560b7907d9b5d69b75605c08530ce6d910a7c829c227f8f7f38ee4e1cbdaa` |

Offsets UTF-16 en `web`: constante 5379489; método 5381759; handler Gateway 5338491; demanda privada 9422146; entrada de respuesta 5316769. Son ubicaciones de texto, no direcciones ni mediciones de ejecución.

La respuesta no lleva nonce de consulta. El controlador conserva el suelo de revisión de toda la cohorte de consultas sin correlación y, cuando hubo cambios intermedios, mantiene un suelo de autoridad ambiguo para esa misma identidad de mensaje. Recibir una respuesta no demuestra que las respuestas anteriores hayan terminado: ni una reconsulta ni otro CALL_CREATE pueden hacer retroceder una observación más reciente. CALL_UPDATE aislado no cuenta como respuesta. Una identidad nueva o un READY_CATALOG fresco —no RESUMED— inicia otra autoridad sin rearmar tráfico automático. Las regresiones cubren ambos órdenes de respuesta, CREATE extra, UPDATE/VOICE_STATE y tombstones.

REA retuvo el análisis `ev_3ecd736169ed7b65dc71c024869f427d02bb5e836f1c0f726c1b3d7c448cb779`, resumen `ev_e17ec1c813b9e7a8126ada8e1a56ce3c20a0a1ff6732369ef1ea7e407f08a9c3`. La selección de dos scripts dejó tres archivos fuera; el analizador del bundle grande agotó su heap y el grafo semántico es parcial. La conclusión del payload se basa en los fragmentos exactos identificados y el enum del script pequeño; no se presenta ese grafo como auditoría completa ni como tráfico observado. La identidad del inventario corresponde al directorio anterior a mover los originales al temporal, sin cambiar sus bytes.

No se añadió una semilla privada supuesta de READY: no se acreditó su payload. La consulta dirigida cubre ese vacío. Una consulta sin respuesta queda incierta y sólo se rearma mediante actualización explícita, hasta cuatro consultas sin respuesta por canal; un canal silencioso no bloquea los demás. Una caída marca la observación como desactualizada y cancela trabajo no emitido; no reproduce consultas antiguas al recuperar. Una observación desactualizada debe actualizarse antes de permitir Unirse, también si cambió mientras se cerraba la llamada anterior. Se mantienen 32 tarjetas, 25 participantes y 64 registros de autoridad. Sólo se reciclan registros sin identidad, tarjeta ni trabajo pendiente; los tombstones y resultados inciertos se conservan. Un conjunto exacto de hasta 4096 canales recuerda consultas automáticas durante la sesión e impide que ese reciclaje las rearme. Al alcanzar límites se conserva la navegación y la carga ordinaria del chat; se informa del fallo opcional sin tráfico nuevo. Actualizar está disponible en DM y grupos.

## Cierre y confirmación

El intento de salida retira al usuario propio de la proyección y publica el snapshot inmediatamente, preservando remotos. La tarjeta y el lateral usan `IncomingCall::is_active()`. Un roster omitido o truncado no demuestra finalización. Un roster completo vacío sin timbre deja de marcar actividad. Los eventos recibidos publican también cuando no hay voz local.

La captura anterior cierra antes de habilitar otra. El proceso posee un permiso de captura desde `spawn` hasta observar su salida, incluso si se cancela su constructor, se abandona OpenSlot o cambia la cuenta. Si no se puede confirmar su fin, se mantiene el cierre seguro. Los receptores de pantalla separados no adquieren ese permiso de entrada.

StreamDelete y VoiceState de salida esperan su completion real del escritor fuera del actor, en orden; no se añade un timeout que libere una operación aún viva. La salida de membership es un paso obligatorio aun si la retirada de stream terminó con error. No hay reenvío ciego. Abrir otra llamada espera fin local, fin de escritura y confirmación de salida. Un ACK anterior al completion no basta; un completion viejo tampoco cierra al reemplazo. En llamadas privadas, un CALL_DELETE aceptado o roster **crudo** completo sin el usuario también puede confirmar la salida pendiente; la proyección optimista nunca cuenta como evidencia remota.

Al cerrar sesión se retira el dueño local, se desecha la salida remota anterior y se eliminan observaciones de voz de la cuenta. Un cierre nativo no confirmado conserva únicamente su barrera local. La caché del registro retiene páginas vacías legítimas dentro del mismo límite LRU de tres canales.

La presencia mantiene barreras de Unknown y RateLimits. Una lectura explícita `get_once` no acredita la identidad de una escritura incierta ni borra su barrera. Si el PATCH confirmó pero falló la emisión de presencia, el reintento explícito del mismo estado repara sólo Gateway. Un estado remoto discrepante ofrece comprobación explícita sin consultar automáticamente ni sustituir la intención reciente; la lectura tardía no pisa una elección nueva. No se drenan elecciones ni actividad RPC al recuperar una operación incierta. Elegir Invisible no modifica el estado de conexión.

## Verificación del primer corte (histórica)

Las pruebas emplean credenciales ficticias, fixtures en directorios temporales, Gateway simulado/loopback y motor falso. No se inició NotDiscord, Discord, una cuenta, captura de pantalla, cámara o micrófono reales. El probe N-API usa exclusivamente dobles de motor y no acredita audibilidad.

Resultados del primer corte, sin sumar suites que se solapan. La continuación siguiente los sustituye para las fuentes actuales:

| Comando / comprobación | Resultado |
|---|---|
| `cargo test -p notdiscord --lib --locked --offline` | **724 correctas, 0 fallos, 5 omitidas**, 48,52 s. Incluye UI, diagnóstico, cierre de procesos, caché, navegación y presencia. |
| `cargo test -p notdiscord --lib controller:: --locked --offline` | **302 correctas**, incluida la respuesta de consulta posterior a CALL_UPDATE/VOICE_STATE_UPDATE y la negativa a unir una observación desactualizada. |
| `cargo test -p notdiscord --lib presence --locked --offline` | **27 correctas**; se solapa con la suite completa. |
| `cargo test -p notdiscord-core -p notdiscord --lib --locked --offline` (núcleo) | **169 correctas** en la ejecución conjunta app/core; no hubo más cambios del núcleo después. Incluye el comando tipado de observación. |
| `cargo test -p notdiscord-media-addon --lib --locked --offline --jobs 2` | **39 correctas**, resultado detallado en la nota de micrófono. |
| Addon `native-probe` + Node con fakes | Build correcto, proceso exit 0, **8 combinaciones de cámara/pantalla y 65 cierres de receptor**, cero recursos finales; `evidence/t2-media-continuity-result.json`. |
| `cargo check --workspace --all-targets --release --locked --offline` | **Correcto**, corte final comprobado en 18,38 s. Comprueba destinos; no genera ni ejecuta un instalador. |
| `git diff --check` y comparación de archivos nuevos con temporal vacío | Sin errores de espacios. Advertencias de conversión CRLF del checkout. |

La primera suite dentro del sandbox encontró tres expectativas antiguas del cierre síncrono y cinco fallos de acceso de fixtures Windows (pipes/junctions). Se corrigieron las expectativas para exigir completion y ACK en ambos órdenes, y las cinco fixtures pasaron fuera del sandbox. Otros dos assertions sólo dependían del adverbio «ya» del mensaje antiguo de llamada terminada; se ajustaron conservando las comprobaciones de ausencia de API, medios y preflight. No se omitió ninguna prueba nueva para obtener el resultado final. Persisten advertencias históricas de código no utilizado y el literal flotante de `server_order.rs`.

Logs en `.tools/temp/current/`: `calls-0110-app-final.log`, `calls-0110-controller-final.log`, `calls-0110-app-core-tests-final.log` y `calls-0110-release-final.log`. El inventario SHA-256 de **24 archivos de producto** modificados o nuevos está en `evidence/t6-source-cut-0.11.0.json`; declara expresamente la base Git y que las fuentes aún tienen cambios sin commit. No se atribuyen a ese corte los documentos o skills previos del usuario.

## Continuación y corte de entrega

El inventario actual es `evidence/source-cut-0.11.0-delivery.json`: 32 archivos de fuentes, manifiestos y drivers, base Git indicada y cambios sin commit. Su SHA-256, usado como `NOTDISCORD_BUILD_ID`, es `5b6286f2043eab9772d2ddd68352bf044f4d4d3ee296f4647b52b684ffba766c`. El inventario anterior de 24 archivos sigue siendo histórico.

- `cargo test --workspace --lib --locked --offline --jobs 2`: **939 correctas** (app 731, core 169, media 39), cero fallos y cinco omitidas de app. Log `calls-0110-full-workspace-tests.log` en `.tools/temp/current/`.
- `cargo test -p notdiscord-media-addon --features native-probe --lib --locked --offline --jobs 2`: **41 correctas**; incluye las dos pruebas de política y correlación del probe. Es una suite que se solapa con las 39 de media, no otras 41 pruebas del workspace.
- `evidence/t2-native-mls-0110-final.json`: motor nativo actual, FakeADM verificado por hash, cinco fases de transporte/OwnStream, callbacks y paquetes de audio crecientes. Sin cuenta, dispositivos físicos ni servicio externo; no acredita un peer DAVE o escucha remota.
- `evidence/t2-native-video-0110-final.json`: **falló** por `FAKE_VIDEO_CALLBACK_TIMEOUT`, cero frames. El ready nativo y el registro del sink fueron aceptados y se liberaron referencias; no acredita captura de cámara ni publicación de vídeo.

Los binarios y el Setup se generaron con features de producción predeterminadas, sin `native-probe`. Build de producción y check de todos los destinos release correctos; paquete verificado y siete comprobaciones IPC correctas sin inicialización nativa. La [nota de entrega local](entrega-local-0.11.0.md) registra identidad de fuentes/binarios, tamaño/hash del Setup y el manifiesto borrador. En ese corte no se instaló ni publicó. No se realizaron pruebas con cuentas ni dispositivos físicos.

Pendiente de confirmar manualmente: tercero que abre una llamada ya activa en el servicio, emisión de presencia visible desde otro cliente, DPI/texto/ventanas de T4 y matriz audible de T2. La última requiere reproducir el incidente antes de atribuir una causa o anunciar una solución.
