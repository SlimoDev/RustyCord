# Plan de modularización y estabilidad de llamadas — 9 de noviembre de 2026

**Estado vigente: T0–T7 implementadas y comprobadas dentro de los límites autorizados; entrega local 0.13.0 generada y verificada.** El apartado 8 y la [nota de ejecución](modularizacion-llamadas-0.13.0.md) registran los resultados posteriores. Los apartados 1–7 conservan el corte de planificación sobre 0.12.0; sus estados pendientes son históricos. No existe todavía una reproducción física que explique las desconexiones del usuario ni que cierre C5/H10.

## 1. Base y alcance

- Checkout: raíz de `NotDiscordRust`; aplicación: `NotDiscord-Rust/`. Rama `NotDiscord`, HEAD **`01c7ed88ae9bcdbe6b4b9288fe6c74fe2c641811`**, manifiesto 0.12.0. Fecha: 9 de noviembre de 2026, America/Lima. Las líneas citadas pertenecen a este corte; los nombres de función son anclas para revalidar después.
- Pedido: revisar el proyecto completo para localizar duplicaciones y responsabilidades que conviene modularizar, mejorar su codificación conforme al desarrollo conservador y volver a investigar llamadas prolongadas/desconexiones aleatorias.
- Voz vuelve a estar expresamente en alcance. La exclusión H06 del plan del 31 de octubre era histórica: no impide esta revisión. Aquel incidente atribuido por el usuario al servidor no prueba el origen de incidentes nuevos.
- Conservar cambios locales anteriores: plan del 31 de octubre y tres archivos de la skill de planificación; archivos de `evidence/` no seguidos. No pertenecen a esta tanda. No se encontró `AGENTS.md` aplicable en los antecesores consultados ni en el repositorio mediante la búsqueda realizada.
- Plus y legacy quedan fuera: refs locales observados `d35c06ac7e824c17198791b3d9208f732bc8be81` y `637d41f897a0e435c3d7e26d1c400afd6a640d0e`. No cambiar variantes ni ramas para planificar.
- Fuentes de autoridad: skill `notdiscord-planificar`, su plantilla/mapa, [desarrollo conservador](desarrollo-conservador.md), [política de versiones](plan-skills.md), [nota 0.12.0](llamadas-presencia-navegacion-0.12.0.md) y planes anteriores. El contrato conservador se consolidó el 4 de noviembre sobre 0.10.1; se contrasta aquí con el código 0.12.0, no se toma su versión histórica como estado actual.
- Tipo propuesto para una futura implementación conjunta: **Mediana, 0.12.0 → 0.13.0**, por correcciones funcionales de llamada y mejoras internas compartidas. Es una inferencia, no una elección expresa de tamaño en este pedido ni un cambio del manifiesto. Revalidar la versión vigente al implementar. No se ha demostrado una vulnerabilidad que justifique clasificarla como Grande.

### Permisos y evidencia

Este pedido autoriza análisis y documentación. No se modificaron fuentes del producto, manifiestos o skills, ni se compiló, ejecutaron pruebas, abrió el cliente, utilizaron cuentas/dispositivos, instaló o publicó. Las comprobaciones de implementación descritas más abajo son propuestas; su ejecución dependerá del pedido de programación y las restricciones vigentes. No inventar una aprobación intermedia si ese pedido ya autoriza implementarlas.

La nota 0.12.0 documenta pruebas sintéticas y entregas anteriores: se consultaron como antecedentes, **no se repitieron ni se atribuyen a esta revisión**. El funcionamiento real prolongado y C5 —pérdida física del micrófono— siguen sin acreditar. Las capturas/diagnósticos antiguos de 0.10.x no son evidencia causal de 0.12.0. **Aclaración posterior del usuario, 9 de noviembre: las desconexiones también le ocurrieron en 0.12.0.** Su respuesta «Sí» confirma esa versión, pero no precisa duración ni si se cortó toda la llamada, el micrófono o el vídeo. Es evidencia reportada, no una reproducción del agente ni confirmación de que H01/H02 sean su causa.

## 2. Cobertura global y límites

El inventario adjunto [fija fuentes, hashes y cobertura](plan-modularizacion-llamadas-2026-11-09-inventario.json). Git contiene 532 archivos seguidos en este corte. Hay **174 archivos Rust bajo `crates/*/src`**: app 125, core 31 y media-addon 18, con 96.278 líneas incluyendo pruebas embebidas. Esos tamaños localizan responsabilidades; no prueban CPU alta ni mala arquitectura. Los tres mayores son `ui/chat.rs` (6.604), `controller/voice.rs` (6.177) y `ui/shell.rs` (4.049).

Se inventarió todo el árbol seguido y se hizo revisión dirigida de arquitectura y recorridos prioritarios. **No se afirma haber auditado exhaustivamente cada línea de los 174 archivos, los 182 archivos de código/configuración vendor ni las dependencias transitivas.** T0 completa la cobertura por responsabilidad antes de declarar cerrada la revisión global. El inventario distingue lectura dirigida de mero inventario; no usa el conteo como certificado de revisión.

| Área | Revisión realizada y dirección del plan |
|---|---|
| Core: API, operaciones, límites y seguridad | Recorrido dirigido de admisión, ticket, emisión, recepción y subida. Ya comparte `SafetyGate`, `Operations`, `RateLimits`, `receive` y límites de cuerpo. Mantenerlos; extracción pequeña H11, sin nuevo cliente HTTP ni retry genérico. |
| Core: Gateway, energía y recuperación compartida | Recorrido de fases, entrega al actor, causas de salida y coordinación. H02/H08. `shared_recovery` ya existe; no crear otro coordinador. |
| Core: modelos, parsing, Markdown, menciones, ACL, catálogos y ajustes | Mapa de módulos y lectura dirigida de contratos compartidos, límites y callers. Ya existen `SharedList`, parser común, permisos y constructores de menciones; continuar caracterización por dominio en T0/T6, sin sustituirlos por helpers paralelos. |
| App: actor, navegación, peticiones y notificaciones | Recorrido dirigido de launch/completion y sus generaciones; H02/H04. Caracterización restante de social, administración, búsquedas, notificaciones, GIFs y actividad/RPC en T0/T6. |
| App: llamadas, fuentes, recuperación y medios | Revisión profunda de los recorridos citados en H01–H04 y H10. Reutilizar fuentes/intenciones y coordinador 0.12; C5 sigue abierto. |
| App: UI, entrada, recursos, visores y shell | Revisión profunda de superficies, foco, visor de texto y políticas de recursos: H05–H07/H09. Resto de chat/editores/menús se recorre por responsabilidad en T0/T6, preservando geometría e interacción. |
| App: autenticación, almacenamiento y datos compartidos | Lectura dirigida de propietarios, leases y helpers compartidos; separación deliberada, no nuevo defecto demostrado. Mantener DPAPI, lectura que conserva errores, escritura atómica y temporales propios. |
| App: diagnóstico y actualización | Lectura dirigida de saneamiento/observación acotada y `UpdateManager`; H08 aprovecha lo existente, no añade logger libre ni gestor paralelo. |
| Multimedia Rust/N-API/IPC y bootstrap JS | Revisión profunda de host, socket, callbacks, ledger y salida de eventos. El JS propio es un cargador declarativo; no convertirlo en segunda implementación de políticas. |
| Compilación, empaquetado, instalador y actualización | Inspección dirigida de reutilización de `scripts/runtime-inventory.ps1`, verificaciones por etapa y helper embebido. Conservar el flujo único. No eliminar controles repetidos que protegen copias distintas. |
| Ejemplos, fixtures, probes, vendor y runtime externo | Inventario; lectura de contratos/probes pertinentes sin ejecutarlos. Vendor tiene parches de entrada Alt+numpad/pegado: revisar diferencias propias, no refactorizar upstream ni atribuir la semántica del motor nativo a nuestros mocks. Sin auditoría nueva de avisos del lock/runtime. |

### Reutilización que ya funciona

- `storage.rs:116–180` centraliza lectura acotada y escritura atómica; `shared_state` lo utiliza. Lectura tolerante y lectura que informa errores tienen contratos diferentes.
- `auth_job`/`auth_lifecycle` separan árbol de procesos y recursos/leases. No fusionar con cachés ni temporales multimedia sólo porque todos tengan `Drop`.
- `updates.rs:108–218` ya coordina comprobación/descarga con `busy`. `install-update.ps1` se embebe autocontenido (`updates.rs:20,599`): no reemplazar su validación por una dependencia del checkout.
- La revisión 0.5 ya incorporó AST compartido, `SharedList`, límites, mantenimiento adaptativo y socket de voz acotado; 0.12 ya incorporó recuperación compuesta, fuentes separadas, speaking común y leases de preview. No volver a presupuestarlos como funciones inexistentes.

## 3. Hallazgos con evidencia

P1 = fallo funcional importante bajo las precondiciones descritas; P2 = robustez/rendimiento/mantenibilidad con impacto concreto; P3 = limpieza acotada. No son severidades de vulnerabilidad. Rutas abreviadas: `app/` = `crates/app/src/`, `core/` = `crates/core/src/`, `media/` = `crates/media-addon/src/`.

### H01 — P1 — Dos detectores del mismo timeout de voz aplican recuperaciones distintas

**Confirmado en fuentes:** `media/host/voice_socket.rs::Heartbeat::sent:176–180` devuelve `VOICE_HEARTBEAT_TIMEOUT` al vencer el plazo pendiente. El lector detecta ese mismo plazo en `reader:463–481`, construye `Failure` y lo clasifica `ResumeCandidate` (`74–79`). `host/recovery_methods.rs::socket_error:130–149` suspende la sesión conservable y permite el recorrido del coordinador.

En cambio, `media/host.rs::tick:2700–2702` ejecuta `beat?`; su bloque de errores llama a `fail:2718–2720`. `fail:1768–1770` emite estado genérico y cierra. El evento no conserva `recoveryClass`/cierre; `app/controller/voice_recovery.rs::fresh_candidate:456–461` no lo reconoce y `voice.rs:3658–3671` termina con `stop_voice`.

**Precondición e impacto:** un tick vencido se procesa antes que el error del lector; el mailbox puede priorizar Tick (`media/host/dispatch_queue.rs:67–69`). Una condición recuperable acaba como final de llamada según el orden de despacho. El watchdog independiente mitiga retrasos JS, pero no unifica estas salidas. La carrera no se reprodujo aquí y no se atribuye todavía a la incidencia del usuario.

**Cambio:** entrada única de fallo de transporte para lector, escritor y heartbeat, antes de cerrar la sesión. Compartir clasificación/contexto; mantener separados errores terminales de DAVE, protocolo y motor. Reutilizar el permiso de recuperación existente, sin añadir intentos ni fallback Resume→Fresh tras fallar.

**Aceptación:** vencimiento con tick primero/lector primero, ACK tardío, cierre duplicado y evento de generación anterior: misma decisión causal, cierre correcto y como máximo la recuperación ya admitida.

### H02 — P1 — Una espera IPC puede bloquear el consumo del actor

`app/controller.rs:258–262` espera `act/complete/dispatch`. `controller/voice.rs::voice_complete:957,960,978` espera directamente órdenes de preparación del host/receptor. `app/media.rs::command:330–345` espera cola y respuesta; `serve_scoped:591–595` escribe al pipe **sin plazo propio**. El timeout de respuesta de 35 s empieza después (`598`).

**Precondición e impacto:** host que no lee/responde. Mientras esas rutas esperan, el actor no consume acciones, ticks ni Gateway. Gateway tiene heartbeats independientes y entrega acotada: canal 256, buffer 32/4 MiB y plazo 5 s (`core/gateway.rs:38,363`, `gateway_runtime.rs:65–95,677`). Con suficiente tráfico puede terminar pidiendo resincronización por saturación local aunque el servidor siga sano.

**Mitigaciones:** sí existe cancelación externa de la tarea mediante `MediaBridge::close` y `select!` exterior (`media.rs:142–144,282–303`), con confirmación de salida del proceso. No es un bloqueo imposible de cancelar. Muchas órdenes usan `launch_media` y ya salen del actor; no rehacerlas indiscriminadamente. El timeout no confirma cierre ni rechazo del comando.

**Cambio modular:** trabajos identificados de preparación/configuración fuera del actor, con un contrato común IPC de admisión, plazo de escritura/respuesta, resultado y cierre. Confirmaciones correlacionadas por cuenta, generación, host/apertura/media epoch. Conservar el dueño hasta que el trabajo/proceso termine de verdad; no sustituir el bloqueo por tareas ilimitadas ni ampliar buffers.

**Aceptación:** host falso que no lee, no responde o responde tarde; el actor sigue procesando Gateway/salida/cambio de cuenta. Una cancelación no permite otra apertura prematura ni aplica la respuesta antigua. Cubrir cierre durante escritura parcial.

### H03 — P1 funcional — El evento de primer frame omite la generación después de recuperar voz

`media/video.rs::metadata:217–218` no incluye `transportGeneration`. `video_attach:358–387` captura el evento `playing` y lo envía directamente por `self.output`, evitando `Host::emit:328–341`, que sí completa esa identidad. `loading` sí utiliza `emit` (`video.rs:448`).

Tras recuperación, `app/controller/voice_recovery.rs:1174–1194` conserva `Phase::Restored`; `stale_voice_transport_event:590–605,620–624` rechaza eventos de la sesión principal sin la generación esperada. `ui/voice.rs:1433–1463` necesita `video.active` para pintar la textura.

**Impacto/certeza:** ruta incompatible confirmada; una cámara de la sesión principal conservada puede seguir en carga aunque lleguen píxeles. No explica por sí sola un corte de audio ni acredita reproducción visual. Los filtros estrictos son una protección, no el problema que se deba retirar.

**Cambio modular:** constructor/envelope de eventos multimedia con contexto explícito, usado también por callbacks directos. Capturar la generación del productor/sink al crearlo; nunca asignar a un callback antiguo la generación actual al llegar tarde. Compartir el forwarding Main/Receiver de `controller/voice.rs:306–319,362–401`, conservando configuraciones y recursos propios de cada host.

**Aceptación:** atravesar el emisor real con `loading → RGBA → playing` antes/después de Resume y Fresh. Evento vigente activa una vez; callback de sink/epoch anterior no reactiva ni libera recursos nuevos. Cubrir detach, pausa, cierre y fallo de receptor sin afectar voz principal.

### H04 — P2 — Los presupuestos de eventos no cubren toda la ruta multimedia

Entrada nativa ya acotada/coalescida: `media/host/dispatch_queue.rs:10–13,43–78`. Pero `host.rs:129` crea `std::mpsc::channel<Value>` ilimitado; `Host::emit:341` y callbacks escriben allí. El hilo pipe escribe síncronamente (`227–238`); `inputLevel` puede producirse cada ≥75 ms (`1924–1958`). En aplicación, `media.rs::emit_scoped_event:459–475` usa salida Tokio ilimitada; los forwarders `voice.rs:308–309,366–378` desembocan en `Completed`, también ilimitado (`controller.rs:231,257–264`).

**Precondición:** consumidor/pipe/actor retrasado mientras continúa la producción. Puede crecer backlog de datos obsoletos. No se midió una fuga ni crecimiento en una sesión normal.

**Cambio:** presupuesto continuo desde productor hasta consumidor. Telemetría reemplazable por último valor por dueño; respuestas, terminales, secuencias de protocolo y DAVE sin pérdidas silenciosas. Saturación de control produce fallo tipado único y cierre seguro. No bloquear el hilo JS con un `send` síncrono ni retirar leases de trabajo aún vivo. Reutilizar contratos pequeños; separar cola de control de slots reemplazables.

**Aceptación:** detener consumidores falsos, generar muchos niveles más control/cierre; cola acotada, último valor útil, control entregado o parada explícita, sin ráfagas de recuperación. Registrar ocupación máxima/coalescencias saneadas. H02 debe corregirse aunque las colas se acoten.

### H05 — P2 — Las superficies modales se enumeran varias veces y ya divergen

`app/ui/overlay.rs:6,55–104,115–153` enumera superficies/IDs/cierre; `dialogs.rs::background_blocked:903–907` mantiene otra lista; `input_ownership.rs::begin:97–165` vuelve a decidir scope/capas/prioridad; `ui/mod.rs:1744–1753` define dibujo. `time_picker` bloquea fondo, posee teclado y se dibuja, pero falta en `dismiss_outside_overlays`; `timestamps.rs:217–276` contempla Escape/cruz/Cancelar/confirmación, sin clic fuera.

**Certeza:** duplicación y disparidad confirmadas; no todas las combinaciones están reproducidas. **Cambio:** descriptor tipado pequeño de superficies activas para orden, capa, bloqueo, foco y cierre; cada formulario conserva su rollback/recursos. No introducir un registro genérico de acciones remotas.

**Aceptación:** tabla de todas las superficies, incluido `@time`, subventana de color y diagnóstico: sólo cierra la superior, clic consumido, popup hijo primero, foco correcto al volver, ajustes restaurados según contrato, sin reentrada al mismo Context egui desde una transacción bloqueada.

### H06 — P2 — El lector de adjuntos crea trabajo nuevo antes de terminar el anterior

`app/ui/text_viewer.rs::start:139–159` crea un thread por apertura; `open_local_text/open_remote_text:179–193` lo inicia antes de cerrar el visor previo. `Load::drop:21–23` sólo marca cancelación. `read_remote:117–132` entra en `client.get(...).send()` sin comprobar esa marca antes; el cuerpo la consulta por chunks (`90–100`).

**Precondición:** aperturas/cierres rápidos con respuesta lenta. Puede acumular threads/GET simultáneos. Hay timeout de **15 s** (`resources.rs::client:370–379`), cuerpo de 1 MiB, cliente sin credenciales/redirecciones y descarte por cuenta/acceso: no es una fuga permanente demostrada.

**Cambio:** ejecutor acotado del visor y lease hasta finalización real; cancelar antes de admitir sustitución, comprobar cancelación antes de emitir, coalescer la demanda visible más reciente. Puede reutilizar admisión/resultado de recursos, conservando su límite/decodificador y permisos propios. No pasar el cliente autenticado ni truncar texto silenciosamente.

**Aceptación:** cien aperturas/cierres con transporte falso bloqueado no producen cien trabajos activos; cancelar no libera capacidad antes de terminar; resultado tardío no sustituye otra ventana/cuenta. Sin reintento automático; contenido local y archivos temporales aún referenciados se conservan.

### H07 — P2 — Expulsar un fallo de caché puede volver a autorizar tráfico

`app/ui/resources.rs::texture_source:1152–1166` admite cuando no hay `failed`; `retry:1186–1188` representa intención explícita. `poll:892–903` expulsa el fallo más antiguo al llegar a 256: revisitarlo vuelve a admitirlo. `resources/custom_animation.rs::texture:354–360` readmite un error tras 60 s; `poll:256–269` también expulsa/limpia fallos. El worker descarga en `initialize:208–243`, línea 230; `Pictures::animated_emoji/end_emoji_section:1360–1388` conecta esta política con demanda visual/hover.

**Certeza/impacto:** reconsulta alcanzable por TTL/evicción, contraria a RC5; no se midió tráfico ni CPU. Son descargas CDN sin credenciales: no afirmar vulnerabilidad o sanción demostrada. Ya hay worker único, canales y animaciones acotadas, fallback y generaciones.

**Cambio modular:** contrato común de admisión y resultado `Pending/Ready/Failed/CapacityDeferred`; política de fallo independiente del contenido expulsable. TTL/repintado no conceden otro intento. Al saturar fallos, usar estado agregado acotado con acción explícita pertinente, nunca un conjunto ilimitado de URLs ni un reinicio global de sesión. Distinguir falta de capacidad local de error remoto y conservar fallback/demanda válida.

**Aceptación:** tiempo virtual >60 s y miles de hovers/repintados tras error no aumentan GET; más de 256 errores no crean ciclo; reintento explícito admite sólo el intento permitido y respeta 429/fin real; cambio de cuenta invalida productores antiguos. Éxitos y demanda fresca siguen funcionando dentro de presupuesto.

### H08 — P2 — Gateway descarta la causa que permitiría explicar los cortes

`core/gateway_runtime.rs:675–679` termina por Hello/Ready/heartbeat; `688,693,713–714` por lectura/EOF/cierre/Reconnect/Invalid Session. Convergen en `798–835` sin motivo tipado. `gateway.rs::ConnectionChange:50–54` sólo transporta fase/espera/resumed; `app/controller/connection.rs:57–62` registra cierre con correlación 0. Voz ya tiene clase/fase/cierre numérico (`media/host/recovery_methods.rs:135–137`).

**Impacto:** los logs no distinguen suficientemente cierre remoto, ACK ausente, escritura atascada o saturación local de entrega. No demuestra que todos hayan ocurrido.

**Cambio:** motivo tipado de salida de Gateway, correlación efímera del episodio y resumen acotado de duración monotónica, fase, antigüedad de ACK y ocupación relevante. Compartir los datos causales entre política/diagnóstico/UI sin cambiar la política por una etiqueta de log. Reutilizar el recorder y schema saneados; sin payloads, IDs privados, URLs, tokens ni close reason de texto libre; sin REST/pings extra/polling.

**Aceptación:** salidas sintéticas diferenciadas/coalescidas, correlación correcta y registros de tamaño/frecuencia acotados. Una falta de evento no se presenta como prueba de conexión sana ni fallo del servidor.

### H09 — P2 — La barrera modal solicita repintados aunque no haya nada que animar

`app/ui/dialogs.rs::draw_modal_barrier:909–929` solicita siempre otro frame a los 16 ms cuando hay superficie bloqueante. `ui/mod.rs:1744–1753` sigue dibujando shell; `shell.rs::navigation_rows:240–274`, `member_rows:304–347` y `friends_visible:12–42` reconstruyen proyecciones antes del recorte de filas (`1000,2239,2453`). La virtualización existente reduce pintura, no todo ese preprocesamiento.

**Certeza:** solicitudes y trabajo repetido confirmados; CPU/energía ahorrables sin medir. No todos esos frames se deben exclusivamente al modal: voz, caret o animación pueden requerirlos. Minimizado ya omite esas superficies.

**Cambio:** la barrera sólo bloquea; eventos/worker/caret/animación/plazos solicitan sus repintados. Después medir si conviene cachear proyecciones puras de shell por revisión, filtros, selección, presencia/permisos y cuenta. No añadir caché arbitraria ni perseguir un objetivo artificial de RAM.

**Aceptación:** modal estático sin multimedia no fuerza por sí mismo un ciclo de 16 ms; caret/spinner/scroll/GIF/resultado asíncrono siguen vivos. Comparar datos y ventanas iguales, frames, CPU y p95; no anunciar un porcentaje de ahorro sin medición.

### H10 — P2 de investigación — Callback nativo ausente puede retener aplicación de frames sin diagnóstico temprano

`media/host/applied_frames.rs:27–55` limita a 128 entradas y sólo avanza el ACK contiguo al completar operaciones, sin edad. `host.rs::callback:382–387` vincula operaciones nativas; `1276–1281,1308–1314` esperan transiciones/propuestas MLS. `Session::update_wait:301–309` retorna si ya está conectada y no impone plazo a ese callback. El watchdog puede seguir recibiendo ACK de heartbeat.

**Confirmado:** límite y ausencia de edad/plazo por operación. **Hipótesis:** que el motor omita un callback en el incidente. El límite 128 y no adelantar `seq_ack` son protecciones correctas, no duplicaciones que quitar.

**Propuesta condicionada:** identificar edad/clase de la primera operación pendiente y confirmar semántica nativa antes de fijar plazos. Si se acredita necesidad, diagnóstico y parada segura por fase; no saltar frames, ACK anticipado, repetir MLS, reinicializar motor o reconectar para ocultarlo. Si exige artefacto externo, investigación específica con la skill REA y acceso permitido; no ejecutar motor/cuenta por este plan.

**Aceptación futura:** callback ausente/tardío/duplicado/epoch anterior y completion tras suspensión; ACK sólo del tramo aplicado, memoria acotada, dueño retenido y ninguna recuperación adicional. Si no hay evidencia suficiente para un timeout nuevo, dejar instrumentación acotada y registrar la investigación abierta.

### H11 — P3 — Normalización del resultado de escritura repetida entre JSON y multipart

`core/api.rs:510–519,608–615` convierte errores sin status `Other/Network` a resultado desconocido; ambas rutas finalizan `Operation`. Es duplicación pequeña confirmada, sin divergencia actual ni ahorro medido.

**Cambio opcional en T6:** helper puro y privado de normalización de resultado, conservando `safe_read`, status/clase, límites de respuesta distintos y `Operation::Drop` cuando la emisión sale antes. No fusionar JSON/multipart completos ni mover la decisión a UI. `get/get_once` y sobrecargas de upload son fachadas con propósito, no duplicaciones que eliminar mecánicamente. Los GET normales admiten hasta tres intentos exclusivamente ante 429 breve confirmado; `get_once` mantiene uno para comprobaciones explícitas. Documentar esa diferencia; no ampliar retries.

**Aceptación:** 401/403-verificación/429/5xx, cuerpo inválido, cancelación antes/después de emitir y confirmación tardía mantienen exactamente sus barreras; subida nunca se repite. Esta limpieza puede diferirse si no reduce complejidad al implementar.

## 4. Instrucciones conservadoras obligatorias para quien programe

Este apartado forma parte del trabajo transferible a Claude u otro implementador; no basta decir «seguir C0». El MD de referencia amplía estas reglas.

1. **Un dueño por cuenta/generación y episodio.** Reutilizar `core/shared_recovery.rs` y el recorrido Gateway → Voice → Viewer de 0.12. Una etapa necesaria en curso, esperar su finalización y parar si falla. Como máximo el intento automático ya permitido tras pérdida; apertura inicial fallida no encadena otro. Coalescer clics, eventos de energía y errores duplicados. No crear loops por host/timer ni Resume fallido seguido de Fresh. Elegir Fresh antes de emitir sólo si el contexto vigente lo justifica.
2. **Mantenimiento obligatorio separado de reintentos.** Hello, Resume/Identify, Ready/Resumed, heartbeat/ACK y cifrado necesarios siguen su protocolo. No suspender heartbeats para cumplir «una operación», ni enviar pings adicionales para diagnosticar. Respetar `Retry-After`, límites globales/ruta y de Identify, además de barreras de Authentication/Verification/Protocol. Los números existentes —30 s de cooldown, 60 s de estabilidad, timeouts/colas— son políticas revisables con fundamento, no cuotas oficiales o cifras ordenadas por el usuario.
3. **Cierre y leases reales.** Un timeout, `Drop` de ventana o cancelación solicitada no demuestran fin de thread, escritura, socket o proceso. Retener propiedad/admisión hasta cierre/confirmación real. No bloquear actor ni hilo JS mientras esperan; no admitir sustitución prematura. Los presupuestos cubren trabajo en vuelo, no sólo lo aún encolado.
4. **Contexto en todo productor.** Cuenta, generación, host/apertura, sesión de medios, transporte, fuente/sink y revisión pertinentes acompañan resultados. Capturar contexto al producir; descartar el antiguo sin asignarle identidad nueva. Mantener aislamiento receptor/voz, ganancias propias, selección explícita, mute/deaf/PTT/VAD y DAVE. No abrir micrófono del receptor ni relajar cifrado/filtros de resultados tardíos.
5. **HTTP opcional detenido durante recuperación.** Invalidar tickets viejos tras pérdida y revalidar después de semáforos/rate waits/antes del primer poll; no reactivar backlog al sincronizar. Conservar confirmaciones ya iniciadas y demanda visible válida. `READY_CATALOG` anterior a `Synchronized` debe seguir difiriendo el catálogo antes de capturar Api/in-flight; crear trabajo fresco una vez sincronizado. No romper de nuevo la carga inicial al endurecer gates.
6. **Mensajes y otras mutaciones conservados.** Este refactor no autoriza reenvíos automáticos al recuperar voz/red, repetir adjuntos ni borrar `Operations.Unknown`/`RateLimits`. Preservar submission/nonce, captura original y borrador posterior, reply/menciones/archivos. Conciliar HTTP/Gateway una vez con identidad correcta; una consulta adicional explícita será acotada y `get_once`, sin polling. Ausencia en página parcial o igualdad texto/hora no prueba no entrega. Permisos actuales y `allowed_mentions` prevalecen.
7. **Recursos con límites y fallos persistentes de admisión.** Nada de HTTP en draw ni un intento por frame/hover; UI sólo expresa demanda al coordinador acotado. TTL/expulsión no rehabilitan errores. No drenar pendientes al recuperar. Mantener límites por bytes, resolución, frames, entradas y tiempo; conservar demanda reciente válida separada del trabajo obsoleto. Capacidad diferida no equivale a error remoto ni licencia para redescargar.
8. **Privacidad y datos.** Medios sin token/cookies Discord; origen/rutas/redirecciones y streaming acotados. Logs tipados locales y exportación voluntaria, sin mensajes, audio, portapapeles, nombres de dispositivos, IDs privados, URLs firmadas, credenciales o dumps. Reutilizar observación optativa y acotada de voz; no stats polling permanente. Preservar datos/sesión/configuración/borradores y limpiar sólo temporales propios sin referencias. No nueva persistencia privada de outbox ni migraciones por refactor.
9. **Cambios mínimos y aceptación completa.** Un módulo comparte una responsabilidad con entradas/estados explícitos, no sólo mueve líneas. No crear un framework de tareas, conexiones o cachés universal, ni juntar HTTP autenticado con CDN, Gateway con voz, o escritura atómica con lectura tolerante. Probar callers completos/arranque/cierre además de helpers aislados; mantener formato IPC y payloads salvo corrección documentada. Separar pruebas propuestas, compiladas, ejecutadas y confirmación real.

La documentación primaria consultada el 9 de noviembre confirma el papel de Hello/ACK/Resume y `seq_ack` de voz v8; sirve para contrastar protocolo, no para extrapolar cuotas de bots a la cuenta del usuario ni para prometer ausencia de sanciones: [Gateway](https://github.com/discord/discord-api-docs/blob/main/developers/events/gateway.mdx), [voz](https://github.com/discord/discord-api-docs/blob/main/developers/topics/voice-connections.mdx), [opcodes y cierres](https://github.com/discord/discord-api-docs/blob/main/developers/topics/opcodes-and-status-codes.mdx). No cambiar aquí los intervalos efectivos sin contrastar Hello, implementación y semántica del servidor.

## 5. Tandas y dependencias

Todas están **pendientes**. No implementar toda la auditoría en un único commit. Completar y validar cada contrato antes de ampliar la abstracción.

| Tanda | Trabajo y entregable | Dependencias y condición de cierre |
|---|---|---|
| T0 — Caracterización global | Revalidar base/diff/inventario y completar ficha por los 174 `src`: responsabilidad, consumidores, estado propietario, I/O, duplicación candidata y decisión compartir/conservar. Agrupar por dominios, no leer todo monolíticamente. Reutilizar revisión T13 anterior revalidando contratos. Revisar parches propios vendor, scripts, ejemplos y feature gates pertinentes; conservar las áreas sin cambio justificado. | Primera. No marcar «todo revisado» por hashes o cantidad. Cada extracción posterior requiere comparación de callers y prueba de comportamiento; registrar áreas todavía sólo inventariadas. |
| T1 — Causa y recuperación de voz | H01 y H08: motivo tipado, clasificación común y diagnóstico saneado. Instrumentación mínima causal antes de medir incidentes. | T0 de esos dominios; orden tick/lector equivalente y un solo episodio. No cambiar presupuesto de red ni timers para disimular el fallo. |
| T2 — Contexto multimedia | H03: envelope/emisor/callbacks y forwarder Main/Receiver compartido, sin compartir sus políticas de captura. | T1 para integración causal; probar identidad end-to-end desde el callback de producción y no sólo JSON inyectado. |
| T3 — Actor y presupuestos IPC | H02/H04: preparación fuera del actor, deadlines completos y control/telemetría acotados desde host hasta actor. | T1/T2 para fallos/contexto. Trabajo en vuelo incluido; cierre confirmado antes de sustituir. No resolver sólo aumentando buffers. |
| T4 — Admisión de recursos | H06/H07: fallos y leases separados de caché, ejecutor acotado para visor; preservar budgets y permisos de cada consumidor. | T0 de UI/recursos. Sin dependencia funcional de cambios de voz; puede desarrollarse en paralelo con dueño separado. |
| T5 — Superficies y repintado | H05/H09: descriptor común, cierre/foco/capas y wakeups por dueño. Medir después si proyecciones de shell merecen caché. | T0 de UI/entrada. Coordinar `ui/mod.rs`/recursos con T4; no editar los mismos archivos simultáneamente. |
| T6 — Extracciones restantes justificadas | Modelo puro de parámetros de vídeo H12 abajo; helper H11 si conviene; candidatos adicionales de T0 en chat/social/notificaciones/GIF/RPC/parsing sólo con duplicación y contrato acreditados. | T0 y contratos de T1–T5 pertinentes. Sin reescritura de dispatchers por tamaño, cambio de framework, actualización masiva de dependencias ni migración de datos. |
| T7 — Larga duración e integración | H10 primero investigación/instrumentación si procede; matriz sintética prolongada, regresiones conservadoras, comparación de recursos y documentación. C5 sólo se cierra con evidencia adecuada. | T1–T6 incluidas, cada resultado distinguido de uso real. Si H10 sigue incierto, registrarlo abierto sin introducir un timeout arbitrario. |
| T8 — Entrega, cuando corresponda | Actualizar estado/notas, check/build y entregables según la autorización de programación. Respetar preferencias de entrega vigentes; publicar sólo si el pedido de programación incluye publicación, siguiendo el flujo de Setup y manifiesto tras asset verificado. | Nunca efecto de este pedido de planificación. Fijar fuentes, build ID/runtime, hash/tamaño/tag, no autorreferencia del Setup ni instalación local implícita; preservar Plus/legacy y `evidence/` ajeno. |

### H12 / T6 — Parámetros de vídeo con proyecciones puras

Duplicación de mantenibilidad P2, sin fallo actual demostrado: `media/host.rs::ready:1375–1379`, `voice_command/startCamera:2436–2449` y `share_capture:2621–2644` construyen/enriquecen streams; `stop_camera:2501`, `video:2589` y `share_capture:2644` proyectan op12 y convierten `rtxSsrc/max_*`. Extraer un modelo validado de stream y **dos proyecciones distintas**, nativa y Gateway, compartiendo campos y validación. No abrir red desde el modelo ni decidir recuperación.

Aceptar con fixtures literales READY/cámara/pantalla con y sin sonido/stop/RID, comparando payload anterior y posterior salvo cambio justificado. Conservar stream de audio, máximos, SSRC/RTX, mute de receptor y DAVE. **No resucitar la hipótesis descartada de que `streamParameters` sustituye todo el audio.** Reducir construcción repetida es mantenibilidad; CPU/RAM ahorradas todavía no medidas.

### Propiedad al delegar

- Un único dueño modifica `media/host.rs` y `host/*`; integrar T1/T2/T3/T6 secuencialmente o con protocolo explícito, nunca cuatro cambios paralelos en ese archivo.
- Otro dueño puede trabajar `app/media.rs`/operaciones del actor, acordando el envelope antes de editar ambos lados.
- UI superficies y UI recursos tienen dueños distintos; `ui/mod.rs` y `controller/voice.rs` se reservan a integración. Core API H11 no requiere tocar red de recuperación.
- Pruebas/fixtures deben ejercer contratos y tener expectativas independientes del helper de producción; no duplicar implementación para generar su resultado esperado.

## 6. Verificación propuesta

### Matriz sintética antes de uso real

| Caso | Resultado exigido |
|---|---|
| Tiempo virtual prolongado con ACK normales, aplicación continua de frames y rekeys | Sin reconexiones periódicas por edad ni crecimiento de ledger/colas; `seq_ack` nunca anticipado. |
| H01 en ambos órdenes, ACK/cierre tardíos | Misma causa/decisión, una etapa activa; terminales permanecen terminales. |
| Host lento/bloqueado/escritura parcial; cierre y respuesta tardía | Actor disponible, plazos y dueño retenidos, presupuesto acotado y sustitución sólo tras terminación real. |
| Resume/Fresh con cámara, pantalla y fuentes simultáneas | `playing` vigente llega desde el callback, identidad antigua rechazada; fuente/receptor no cierra voz principal. |
| Gateway y voz fallan juntos; suspensión/reanudación duplicada; varios clics | Coordinación Gateway→Voice→Viewer, sin intentos paralelos ni secuencia de reintentos oculta. Restaurar sólo intenciones explícitas vigentes. |
| Cambiar cuenta/canal/apertura durante cualquier trabajo | Resultado antiguo no aplica ni libera recursos nuevos. Mantener cancelación y permisos actuales. |
| Saturación de telemetría/control y fallos de recursos | Último valor reemplazable, control no perdido silenciosamente, fallo único; sin GET por TTL/evicción/hover ni reconsulta masiva. |
| Regresión HTTP conservadora | Doble clic, 429/plazos mayores, tarea esperando/sin primer poll, pérdida tras emitir escritura, confirmaciones HTTP/Gateway en ambos órdenes: sin duplicados, barreras inciertas retenidas. |
| Carga inicial parcial `READY_CATALOG` antes de `Synchronized` | Demanda retenida, una lectura fresca tras sync, listas cargan; ticket antiguo no se rehabilita. |
| Tabla de modales/pegado/teclado/scroll | Prioridad y cierre coherentes, Ctrl+V al editor permitido, Enter/Shift+Enter y Alt+numpad conservados, clic no atraviesa al fondo. |

Empezar por tests puros/dobles/relojes controlados del recorrido afectado. Antes de ejecutar una suite/probe, comprobar que no abre cliente, motor de terceros, red externa, cuenta, dispositivos o instalador fuera del alcance vigente. Usar los tests de producción pertinentes y regresiones significativas; no ejecutarlo todo repetidamente por rutina. Compilar una prueba no equivale a ejecutarla. Las pruebas sintéticas aceleradas no acreditan audibilidad ni comportamiento del runtime físico durante horas.

### Observación prolongada real, pendiente de autorización o evidencia del usuario

Propuesta de ventanas comparables de **2–4 horas**, duración de ensayo y no límite oficial: voz sola; voz con pantalla; con cámara/ambas; visible/fondo/minimizada; tráfico de chat y carga elevada; pérdida/reanudación de conectividad y energía cuando esté permitido. No introducir cortes reales ni suspender el PC durante esta planificación. Reutilizar observación optativa de hasta diez segundos de 0.12 para C5, sin grabar ni consultar estadísticas permanentemente.

Registrar build/variante, duración monotónica, fase/causa, edad del último ACK, latencia del actor, máximos de colas/control/telemetría, trabajo en vuelo, ledger aplicado, contadores saneados y terminación de propietarios. Distinguir transporte Gateway, voz, cifrado, captura de micrófono, recepción remota y presentación de vídeo. Capturar intención/puerta/deltas **no demuestra audibilidad remota**. No descartar al servidor como causa si sólo se conoce que se cortó la llamada.

Para rendimiento, comparar mismo contenido, hardware, condiciones, procesos y número de ciclos; reutilizar el protocolo de [baseline](profile-baseline-0.5.0.md) y el helper numérico existente cuando esté autorizado. Medir CPU por segundo/frame, p50/p95, private committed bytes, working set, hilos/handles y host auxiliar por separado. Una reducción de working set con commit similar puede ser recorte de residencia de Windows; no prueba una fuga corregida. Cachés y GPU son presupuestos parciales, no RAM total. No forzar `EmptyWorkingSet` ni prometer volver a 35 MB.

### Recuperación del cambio y entrega

Commits por contrato con pruebas/evidencia pertinente; sin migraciones de datos previstas. Si una extracción falla, revertir esa tanda manteniendo protecciones previas; rollback de UI no revierte operaciones ya aceptadas remotamente. Un permiso antiguo de publicación o una prueba histórica no autoriza ejecución nueva fuera de su alcance. Si la programación autorizada incluye publicación, usar los scripts existentes, asset único Setup, fuentes/runtime fijados, verificación remota de tamaño/SHA-256/tag antes del manifiesto, título `NotDiscord x.x.x` y conservación de la instalación del usuario salvo pedido expreso.

## 7. Estado para continuar

- [x] Localizar revisión/variante, leer skill y contrato conservador; inventariar el árbol seguido.
- [x] Revisar arquitectura global y profundizar rutas con evidencia; contraste independiente de H01 y matices de cancelación H02.
- [x] Separar defectos confirmados en fuentes, hipótesis físicas, endurecimiento y limpieza sin ahorro medido.
- [x] Guardar plan autosuficiente e inventario en `docs/`; comprobar referencias y estabilidad del corte.
- [ ] Completar caracterización exhaustiva por responsabilidad T0 y aplicar sólo extracciones justificadas.
- [ ] Implementar T1–T7, ejecutar comprobaciones dentro del alcance autorizado y registrar resultados reales.
- [x] Registrar que el usuario también sufrió desconexiones en 0.12.0; no tratar el problema como exclusivo de versiones antiguas.
- [ ] Precisar duración y parte afectada, y confirmar la causa de los cortes y C5 con evidencia adecuada o ensayo autorizado; no marcar llamadas «resueltas» por fixtures.
- [ ] Entrega/publicación T8 según el pedido de programación; este plan no la ejecuta.

No quedan decisiones que impidan empezar la futura implementación por T0/T1. Las aclaraciones del usuario sobre versión/tiempo/síntoma afinarán la reproducción; no cambian la obligación de conservar el contrato conservador.

## 8. Ejecución posterior — 0.13.0

El usuario pidió ejecutar este plan con `notdiscord-programar`. Se adoptó la
clasificación **Mediana → 0.13.0** por correcciones funcionales y contratos
internos. La preferencia permanente del usuario incluye publicar fuentes,
instalador y manifiesto al completar los controles permitidos, sin una nueva
confirmación. No se amplió la autorización a cliente, cuentas, dispositivos,
suspensión real o instalación.

Al iniciar la implementación el usuario precisó: «Como 15 minutos para arriba
y siempre soy yo». Es evidencia reportada de duración y afectación propia;
continúa sin precisar qué transporte/captura se pierde. No identifica una causa
ni justifica un temporizador nuevo de quince minutos.

| Tanda | Resultado de implementación/comprobación |
|---|---|
| T0 | 179/179 fuentes actuales de `src` caracterizadas; 11 contratos auxiliares, evidencias heredadas y lecturas actuales distinguidas |
| T1 | Clasificación de heartbeat compartida y causas Gateway tipadas; registro sin ocupar Delivery ni cambiar recuperación |
| T2 | Identidad del sink capturada y forwarding compartido; N-API real con objetos falsos confirma RGBA/generación/rechazo antiguo |
| T3 | Preparación fuera del actor, admisión IPC sin espera, deadlines completos, permisos hasta consumo y cierre real; saturación explícita |
| T4 | Visor activo1/pendiente1 con lector falso bloqueado y cien sustituciones; fallo independiente de caché/TTL y Retry-After conservado |
| T5 | Superficie compartida, prioridad/foco/cierre exterior `@time`, barrera sin repintado16ms; fixtures headless correctas |
| T6 | Modelo de vídeo con proyecciones distintas; H11 opcional diferido por beneficio marginal, sin fusionar JSON/multipart |
| T7 | Matriz sintética y cuatro horas virtuales correctas; diagnóstico acotado de ledger. C5/H10 y llamadas reales siguen abiertos |
| T8 | Build/check release, paquete y siete IPC correctos; Setup generado sin ejecutar. Publicación después de verificar el asset |

Pasaron **1.021 pruebas del workspace** (app784, core178, media59; cinco ignoradas
existentes) y **61 diagnósticas**, solapadas con media. Los dos drivers N-API
pasaron usando sólo objetos falsos, sin motor externo, red, cuentas ni
dispositivos. Las correcciones de fixtures de apertura asíncrona y consumo de
salida acotada no retiraron expectativas de error, limpieza, cierre ni saturación.
La [nota 0.13.0](modularizacion-llamadas-0.13.0.md) y el
[informe T0](../evidence/t0-review-0.13.0.json) distinguen alcance y ejecución.

Los contratos se registraron en commits separados: `413ef27f` para voz/IPC y
`abf5704a` para recursos/superficies. El corte final fija 189 entradas de fuentes
y contratos, con build ID
`61bc8d3708e4dc9fed61448e6039e1da75ede2eea6079f2d7c5bd9800137637b`.
No se midieron CPU/RAM/p95 ni se anuncia ahorro; se conservaron políticas,
datos, borradores, dependencias y variantes ajenas.
