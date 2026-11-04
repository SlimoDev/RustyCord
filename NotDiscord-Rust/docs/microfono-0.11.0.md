# Continuidad del micrófono al publicar vídeo — 0.11.0

T2 del plan del 4 de noviembre de 2026. Base: `865b2f11069fadcd40f40bc634e7162ea7694f3e`, variante NotDiscord. La pérdida de micrófono es un incidente comunicado por el usuario; no se ha reproducido con cuentas ni dispositivos. Esta revisión no acredita todavía su causa nativa ni audibilidad remota.

## Evidencia y decisión sobre los parámetros de streams

`Host::ready` crea audio más capacidad de vídeo; `startCamera` y `share_capture` actualizan los parámetros de vídeo. El wrapper empaquetado `media-host/runtime/resources/standalone_modules/discord_voice/index.js` sólo reenvía `setTransportOptions` al objeto nativo. Su SHA-256 es `1c12139ccbf818a992fb813b4954a68b1d64b4e96b0f6a97e12862ca5eef764a`.

REA analizó estáticamente ese wrapper, sin ejecutarlo: evidencia de análisis `ev_5e7b253d01c01d45dff130613aeb20708a4daff9dbff61dfaa3f5505cc5d3771`, resumen `ev_b78207932dbf001adf43d2347a6fe05804573c9e972cee12134c5477ef80fcd7`. Sólo se seleccionó `index.js`; quedaron tres archivos de inventario fuera del análisis y las relaciones dinámicas permanecen desconocidas. REA abrió `discord_voice.node`, SHA-256 `7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607`, pero el proveedor nativo no estaba disponible: Ghidra e IDA sin configurar, Hopper no soportado en Windows. Se cerró la sesión. No se instaló ni reconfiguró un proveedor.

Una lectura ordinaria del artefacto web histórico ya disponible en `.tools/temp/current/gif-060-static/web.ad0b95d8568e0f6b.js`, SHA-256 `3f215e2a4d156120449c090a08973d2e7d96da0d9736a4bb79928829a0dfd4e0`, muestra que el cliente también envía deltas que sólo incluyen vídeo: `updateVideoQuality` alrededor del offset UTF-16 10949750 y `setStreamParameters` alrededor de 11015471. La inicialización incluye audio alrededor de 10995110. Es evidencia estática histórica del contrato esperado, no prueba de ejecución ni de la semántica interna del `.node` actual.

Por ello se conserva la lista de parámetros: no se añade audio a cada delta basándose únicamente en la hipótesis de reemplazo. Cámara y Go Live siguen usando sus sesiones y SSRC propios; ningún cambio de vídeo abre el micrófono en Go Live ni reinicia la conexión principal.

## Diagnóstico implementado

`Session::input_diagnostics` separa:

- callbacks de entrada procesados y edad de la última muestra;
- elegibilidad local, VAD/PTT y tecla PTT;
- puerta nativa, transporte y cifrado de la voz principal;
- contadores de salida de audio, conservados por el saneamiento existente de `stats`.

Los snapshots de fases `camera-selected`, `camera-capture-requested`, `camera-stopped`, `share-selected`, `share-capture-starting`, `share-capture-requested`, `share-capture-cleared` y `video-session-closing` llevan `voiceInput`. La respuesta existente de estadísticas añade `input` a la voz principal. No hay polling nuevo, grabaciones, nombres de dispositivos, IDs privados ni modificación de captura para obtener el diagnóstico. `remoteAudibilityVerified` permanece explícitamente falso.

El adaptador de `app/src/diagnostics.rs` transforma sólo las etapas y campos tipados permitidos en `VoiceInputState`, disponible en el ring/exportación existente. Nunca entrega el JSON arbitrario al grabador. Su correlación numérica usa el esquema 1 en el byte superior: flags 0–10 para muestra obsoleta, elegibilidad local, PTT pulsado, puerta nativa, transporte, cifrado, elegibilidad de transmisión, mute, deaf, prueba local y ganancia cero; bit 11 para modo PTT, 12 para callbacks no nulos y 13 para edad conocida. El byte desde el bit 16 identifica la fase (1 compartir seleccionado, 2 cámara seleccionada, 3 captura cámara solicitada, 4 cámara parada, 5 captura pantalla iniciándose, 6 solicitada, 7 retirada, 8 sesión de vídeo cerrándose, 9 conectada, 10 nuevo epoch MLS). Desde el bit 24 se guardan 16 bits de callbacks y desde el 40, 16 bits de edad en ms, ambos saturados a 65535. Edad ausente se distingue por el bit 13; se rechazan tipos inválidos y fases desconocidas. Los contadores de salida continúan en el comando `stats` existente, sin añadir consultas automáticas ni afirmar que estén en este resumen de fase.

El contador se mantiene durante un rekey mientras que la muestra vigente y la puerta se revocan con el contrato existente. Esto permite distinguir una interrupción de callbacks de un cierre deliberado por cifrado, mute, deaf, prueba local o ganancia cero.

## Ensayo sintético

El entrypoint opcional `probeReceiveLifecycle` utiliza exclusivamente objetos N-API falsos. Su transporte de prueba comparte el outbox acotado y la cancelación reales, sin DNS, TLS ni sockets externos. Se repararon fixtures que todavía utilizaban un Sender y un socket anteriores, que asumían visualización automática de pantalla y propagación de ganancia a receptores que ahora se configuran separadamente. Una aserción fallida libera sus referencias N-API antes de devolver el error.

La matriz añadida recorre VAD/PTT, sonido de pantalla activado/desactivado y cámara antes/después de pantalla: ocho combinaciones. Invoca las rutas de publicación ya negociada de Host, cambia la fuente, para pantalla/cámara y comprueba errores de pantalla y callbacks tardíos. Verifica que no cambien el dispositivo/callback/ganancia/puerta del micrófono principal ni su cifrado y que Go Live conserve su mute de micrófono. También verifica que `stats` mantenga separados los datos de entrada y los contadores nativos saneados. Los contadores suministrados por el falso son sintéticos; no demuestran codificación, paquetes reales ni entrega remota.

Comprobaciones ejecutadas el 4 de noviembre de 2026:

- `cargo build -p notdiscord-media-addon --features native-probe --locked --offline --jobs 2`: correcto; avisos existentes de código no utilizado.
- `cargo test -p notdiscord-media-addon --lib --locked --offline --jobs 2`: **39 pruebas correctas**, incluida separación de entrada/puerta/cifrado y ausencia de datos privados. El ejecutable de pruebas avisa que no ofrece símbolos Node-API; las pruebas puras no los invocan y terminaron correctamente.
- Node 24.19.0 ejecutó `evidence/t2-media-continuity.cjs` contra el addon diagnóstico recién compilado: **Success = true**, ocho combinaciones nuevas, 65 receptores cerrados, cero sesiones/conexiones/sockets/sinks finales. Proceso finalizado con código 0, sin cargar el motor nativo ni abrir dispositivos, red real o cuentas.
- `git diff --check` de los archivos modificados: sin errores de espacios.

El resultado completo es `evidence/t2-media-continuity-result.json`, con SHA-256 del addon diagnóstico y las cuatro fuentes observadas. El script reproducible recibe el `.node` diagnóstico y el destino del JSON; nunca importa `discord_voice` ni llama a `run()`. La prueba sintética incluye el inicio de transporte/handshake mediante fakes y las rutas de publicación ya negociada; no ejecuta `startShare` contra un servidor real. Los dos tests del adaptador de diagnóstico pasaron dentro de la suite integrada final de la aplicación (724 correctas, cinco omitidas).

## Confirmación funcional pendiente

El próximo ensayo real autorizado debe comparar voz sin vídeo con inicio, parada y cambio de pantalla/cámara, correlacionando los cuatro grupos de evidencias anteriores. Si cesan callbacks, investigar captura/callback global; si continúan pero se cierra la elegibilidad/puerta, revisar VAD/PTT, controles y DAVE; si la puerta permanece abierta pero no progresan los contadores de audio, investigar publicación nativa; si progresan, contrastar recepción remota. No se repetirá señalización ni se abrirá una segunda captura automáticamente para diagnosticarlo.

Un medidor o contador local no acredita audibilidad. En ese primer corte estaban pendientes tanto la confirmación con otro participante como la semántica nativa de `streamParameters`. La continuación siguiente resuelve esta última, sin declarar solucionado el incidente del micrófono.

## Continuación ABI estática — pedido «ejecuta todo»

La investigación posterior, sin cargar el motor, permite resolver una de las incógnitas del corte anterior: **en este `.node`, un delta `streamParameters` de vídeo no elimina por omisión el descriptor de audio existente**. Se siguió desde el registro JS de `setTransportOptions` hasta `VoiceConnectionWrapper::SetTransportOptions`, el export `Connection::ApplySettings`, su tarea de conexión y la rutina que aplica parámetros de transporte. Ésta itera descriptores por tipo y, para vídeo, busca/actualiza o añade por RID. El objeto de audio sólo se modifica al recorrer una entrada audio o propiedades pertinentes suministradas expresamente.

REA sigue sin proveedor nativo configurado, por lo que no se repitió la decompilación bloqueada ni se instaló uno. Se usó el `dumpbin` 14.44.35207.1 ya disponible y un índice PE con la biblioteca estándar del Python existente. Son resultados ordinarios de shell y una deducción estática; no se presentan como Evidence de decompilación REA ni como prueba de ejecución. El artefacto conserva el SHA-256 `7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607`.

La cadena y las direcciones verificadas, con límites de cobertura, están en [el informe ABI](../evidence/t2-abi-findings.md) y `evidence/t2-abi-manifest.json`. El índice reproducible `evidence/t2-abi-string-index.json`, su script y los desensamblados acotados `evidence/t2-abi-*.asm` se conservan como artefactos locales no versionados; el manifiesto registra sus huellas. La conclusión se basa en las instrucciones del artefacto actual, no sólo en el caller JavaScript histórico citado arriba.

Esto respalda conservar los deltas de vídeo actuales. Sigue sin acreditar la causa del incidente real: no descarta que crear/cerrar OwnStream afecte al engine global, ni una transición de callbacks, puerta o DAVE. El alias `createOwnStreamConnectionWithOptions` del wrapper usa la misma fábrica `VoiceConnection`; su nombre no garantiza aislamiento. El próximo ensayo nativo sintético debe observar esa transición y las estadísticas saneadas de la voz principal sin dispositivos físicos ni servicio real. No se declara resuelta la audibilidad del micrófono a partir del análisis estático.

### Flags nativos de captura ficticia

El mismo análisis estático confirma que `initialize` consume `useFakeAudioCapture` y `useFakeVideoCapture` en este hash. Se siguieron sus bytes desde el parser `0x1800053b0`, a través de `EngineConfig`, hasta la selección de backend. Audio verdadero llega a la factory `fake_adm.cpp` (`0x1800de980`) y a un productor de muestras local; funciona con y sin offload de controles. Vídeo verdadero selecciona `Strobe Video Source`, evitando la rama alternativa que construye el backend físico. La cadena con direcciones, copias y límites se agregó al [informe ABI](../evidence/t2-abi-findings.md) y al manifiesto de evidencias.

Esto acredita el consumo nativo de los flags para esas rutas concretas; no supone una auditoría completa de todas las interacciones de inicialización con Windows. Durante ese análisis no se cargó el motor ni se ejecutó el probe. La ejecución posterior se describe a continuación.

## Continuación nativa sintética ejecutada

El addon diagnóstico se compiló con `native-probe`; sus 41 tests pasaron. El driver `Test-NativeMls.ps1 -VerifiedFakeBackend` verifica el SHA-256 exacto del `.node` antes de habilitar una excepción privada del probe: el identificador lógico `default` corresponde al FakeADM cuya fábrica se acreditó en el análisis estático, no al backend físico. Sin esa verificación sólo se admite una entrada con identidad sintética explícita del catálogo. Inicialmente entrada, salida y vídeo están deshabilitados; la salida permanece a volumen cero. No se accede a cuentas ni dispositivos físicos.

`evidence/t2-native-mls-0110-final.json` registra ejecución del motor real con UDP limitado a localhost y claves sintéticas no persistidas. Se obtuvo un key package MLS de 393 bytes. Las cinco mediciones nativas fueron:

| Fase | Callbacks de entrada | Paquetes de audio | Frames capturados |
|---|---:|---:|---:|
| Base | 4 | 11 | 24 |
| Delta de vídeo | 7 | 22 | 47 |
| OwnStream creado | 10 | 35 | 71 |
| OwnStream destruido | 14 | 46 | 94 |
| Descriptor completo restaurado | 17 | 57 | 117 |

`NativeCaptureContinuityVerified` y `NativeAudioContinuityVerified` son verdaderos; las referencias nativas se liberaron. Las estadísticas son del motor, mediante `getFilteredStats`, no contadores inventados por un doble. Cada fase tiene una espera acotada; un callback inválido o ausente detiene consultas posteriores y nunca se interpreta un contador ausente como cero exitoso. Un primer intento con `getStats` no entregó callback y agotó el watchdog; se conserva en el temporal `t2-native-0110-full/mls-unrestricted/`. No fue una reproducción del fallo del producto.

El ensayo comprueba el delta de transporte y la creación/destrucción de OwnStream. **No publica una cámara, pantalla ni ventana, no establece un peer DAVE y no acredita escucha remota.** `RemoteAudibilityVerified` sigue siendo falso. Aceptación del setter PTT tampoco equivale a leer la puerta nativa de transmisión.

El ensayo separado de vídeo con entrada ficticia terminó con `FAKE_VIDEO_CALLBACK_TIMEOUT`, cero frames y liberación de referencias. Se conserva como fallo en `evidence/t2-native-video-0110-final.json`; no se suma como validación de captura ni como causa del incidente de micrófono. Sus campos ready/sink corresponden a un callback RGBA invocado manualmente sobre otra identidad, no al productor nativo esperado. `InventoryCount=0` no distingue array vacío de resultado no array; el fixture eligió una identidad de respaldo inventada sin correspondencia acreditada con Strobe. Además, `useFakeVideoCapture=true` toma la rama Strobe y omite el lector Y4M en el artefacto inspeccionado (`t2-abi-video-factory.asm`); `SyntheticFileConfigured` sólo acredita haber suministrado una opción. Estos límites del fixture impiden interpretar el timeout como fallo de cámara o conversión del producto.

Se descarta la hipótesis concreta de que omitir audio en un delta de vídeo lo elimina. La revisión de Rust tampoco mostró una causa directa demostrada: las rutas Go Live protegen los controles de entrada con `!stream`; cámara modifica vídeo; el ACK de configuración rearma correctamente la tecla PTT; los callbacks de micrófono conservan servicio bajo carga de órdenes. No se añadió una corrección especulativa. T2 continúa pendiente de reproducción con micrófono físico y recepción remota en una llamada autorizada; el diagnóstico y el binario local permiten realizar esa comparación. No se realizó ese ensayo con cuentas ni dispositivos físicos.
