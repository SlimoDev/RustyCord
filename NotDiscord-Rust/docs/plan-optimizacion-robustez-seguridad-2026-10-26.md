# Plan de optimización, robustez y seguridad de NotDiscord

Fecha: 26 de octubre de 2026, America/Lima. Estado: análisis original conservado; implementación 0.5.0 y comprobaciones registradas en «Seguimiento de implementación autorizado» al final.

La prioridad es corregir el ciclo de vida de las conexiones de voz, acotar entradas y colas, y evitar trabajo repetido al presentar mensajes. La revisión de autenticación y API no confirmó una vía remota de extracción de cuentas. Sí hay retención de credenciales cifradas y oportunidades de endurecer el empaquetado y las actualizaciones. No se ha demostrado que los 60–90 MB observados por el usuario sean una fuga.

## Alcance y punto de partida

- Repositorio: `E:\DESCARGAS\REA\NotDiscordRust`; aplicación en `NotDiscord-Rust/`.
- Variante: NotDiscord estándar; rama `NotDiscord`.
- HEAD observado: `bca0f0f895ad09f740066dd09dadd3dc6e564f32`.
- Manifiesto observado: `version.json`, versión `0.4.1`, esquema 2. La versión `0.2.0` de Cargo no sustituye la versión comercial del manifiesto.
- Se revisaron fuentes locales actuales, incluidos cambios ajenos a esta auditoría. El checkout recibió cambios concurrentes en controlador, chat, diálogos, API y límites de subida; antes de implementar hay que contrastar funciones y diff otra vez. Las líneas citadas son orientativas de este corte; los nombres de función son las anclas estables.
- No se modificaron fuentes del producto, datos, instalación, cuentas ni binarios para elaborar este plan. No se cambió de rama, compiló, publicó ni ejecutó el cliente o el instalador.
- Se conserva NotDiscord+ y su estado anterior. Revisar la variante estándar no permite extrapolar los resultados a Plus ni integrar ambas ramas.

Las rutas de evidencia se expresan respecto a `NotDiscord-Rust/`. Las abreviaturas `app/src/`, `core/src/` y `media-addon/` corresponden a `crates/app/src/`, `crates/core/src/` y `crates/media-addon/`; `ui/` sin prefijo se refiere a `crates/app/src/ui/`.

El pedido del corte original autorizaba análisis, archivo del plan y creación de `notdiscord-planificar`. La petición posterior ya autorizó implementar; su alcance y entrega se registran al final. Las restricciones de cuentas, dispositivos y pruebas funcionales recogidas en README y `docs/updates.md` permanecen vigentes. Las pruebas descritas más abajo son criterios futuros, no resultados obtenidos.

**Versionado propuesto para una implementación conjunta:** Grande, por incorporar endurecimiento de credenciales y cadena de distribución; desde la base observada sería `1.0.0`. Es una propuesta de alcance, no una vulnerabilidad crítica confirmada ni un cambio de versión realizado. Recalcular desde el manifiesto vigente al implementar. Una tanda independiente limitada a rendimiento/robustez debe clasificarse por su alcance real conforme a `docs/plan-skills.md`.

## La fluctuación de 60–90 MB a 29–35 MB

El usuario observa normalmente 60–90 MB, y en ocasiones unos 35 MB —mínimo aproximado de 29 MB— durante mucho tiempo. Una captura muestra `NotDiscord.exe`, 87,6 MB y 0,7 % de CPU. Esa captura no identifica el contador exacto, el árbol de procesos ni el contenido visible; tampoco permite atribuir el cambio al arranque.

Hay dos mecanismos compatibles con esa observación que deben distinguirse:

1. **Residencia en RAM administrada por Windows.** El working set contiene páginas residentes; Windows puede retirar páginas inactivas sin que el programa haya liberado sus asignaciones. La memoria comprometida privada puede permanecer estable mientras baja la columna de RAM. Esto puede explicar por qué una cifra baja se mantiene en segundo plano. Es una hipótesis razonable, no la causa confirmada de este caso. Véanse [working set](https://learn.microsoft.com/en-us/windows/win32/memory/working-set) y [diferencia entre commit y private working set](https://blogs.windows.com/msedgedev/2021/01/13/investigate-microsoft-edge-memory-usage/).
2. **Liberación explícita de recursos visuales.** `Pictures::set_visual_suspended`, en `crates/app/src/ui/resources.rs:1185`, llama a `stop()` y limpia animaciones al minimizar. `stop`, en la línea 1173, libera animación y vista grande; `finish_frame`, en la línea 1329, aplica la suspensión y detiene un GIF que dejó de verse. `ui/mod.rs:933–939` libera texturas de vídeo y drena fotogramas al minimizar. `resources/custom_animation.rs:400–417` retira animaciones no solicitadas en el frame. Las texturas ordinarias de la LRU no se vacían todas por ese camino. Perder el foco y minimizar son estados distintos. El asignador y el controlador gráfico pueden conservar memoria después de liberar objetos; no hay equivalencia automática entre bytes de caché y RAM visible.

La fuente revisada no contiene llamadas a `EmptyWorkingSet`, `SetProcessWorkingSetSize` ni un temporizador que intente dejar el proceso en 35 MB. No se identificó un disparador único de la bajada. Los contadores del proceso vivo no estuvieron disponibles desde el contexto de esta auditoría; no se obtuvo una serie temporal nueva.

**Diagnóstico propuesto:** registrar a intervalos moderados PID, hora de creación y PID padre; working set, private working set y private committed bytes; CPU acumulada y normalizada; fallos de página, hilos, handles, memoria GPU dedicada/compartida cuando esté disponible, número de procesos multimedia y sus métricas por separado. Correlacionar únicamente estados como visible/minimizado, foco, llamada activa, GIF activo, visor abierto y tamaños agregados de cachés. Evitar texto, URLs privadas, IDs de cuenta, tokens, nombres de ventanas y volcados de memoria. Reutilizar la lectura de contadores de `crates/app/examples/emoji_open_probe.rs:165–207`, sin ejecutar ese probe: trata un contador privado no disponible como `null`, y sus metadatos de texturas no miden residencia VRAM. No sumar working sets totales de varios procesos como si las páginas compartidas fueran exclusivas.

El host `NotDiscord.Media.exe` se inicia aparte (`media.rs:133–150`) y tiene cierre acotado (`media.rs:193–205`, `controller/voice.rs`, `stop_voice_internal`). Su desaparición altera la suma del árbol de procesos, pero no explica directamente una fila individual de `NotDiscord.exe`. Confirmar si la captura representa proceso o agrupación antes de comparar.

| Cambio observado | Interpretación que se debe comprobar |
|---|---|
| Baja working set; commit permanece parecido; al volver suben fallos de página | Recorte de residencia de Windows compatible con memoria aún asignada |
| Baja commit junto con bytes de animaciones/visor | Liberación de asignaciones compatible con limpieza del cliente |
| Desaparecen procesos auxiliares | Cambia el conjunto de procesos medido; comparar cada PID y el total con el mismo criterio |
| Commit crece repetidamente tras volver al mismo estado y no se estabiliza | Investigar retención/fuga; una fluctuación aislada no basta |

No fijar 35 MB como objetivo universal ni forzar recortes de working set para mejorar el número mostrado. La aceptación es menos trabajo y memoria comprometida innecesarios, con recuperación rápida al volver a la app. Comparar foreground, segundo plano sin minimizar y minimizado; mantener por separado escenarios sin llamada y con multimedia. Su ejecución necesita respetar las restricciones vigentes; el usuario puede aportar observaciones de su uso normal sin iniciar sesiones de prueba desde el agente.

## Hallazgos confirmados y medidas propuestas

P1 indica un fallo que conviene corregir primero; P2 una mejora relevante con precondiciones conocidas; P3 endurecimiento o coste cuyo impacto necesita medición. Estas prioridades no son puntuaciones CVSS. «Confirmado» se refiere al flujo del código, no a reproducción funcional o explotación.

### R1 P1 El timeout de heartbeat de voz se renueva sin recibir ACK

**Evidencia:** `crates/media-addon/src/host.rs`, `beat`, líneas 1704–1724; `tick`, 2597–2598; procesamiento de ACK, 1127.

`beat()` comprueba si el heartbeat pendiente supera dos intervalos y después reemplaza su instante por `now`. Con ticks normales cada intervalo y sin ACK, su antigüedad vuelve a empezar antes de alcanzar el timeout. Una llamada puede conservar señalización y recursos pese a que el servidor dejó de confirmar.

**Cambio:** mantener la fecha del primer heartbeat no confirmado hasta su ACK, o representar explícitamente enviado/confirmado. El reenvío no debe extender el plazo. Invalidar temporizadores al cerrar.

**Aceptación:** con reloj sintético, varios intervalos sin ACK terminan la sesión; ACK a tiempo conserva la conexión; cerrar o reemplazar sesión impide que actúen temporizadores antiguos.

### R2 P2 La cola de salida de voz no está acotada y el cierre espera al escritor

**Evidencia:** `host.rs:1054`, `unbounded_channel<Message>`; `write.send(...).await` sin deadline, 1067 y 1082; `close`, 1763–1764.

Con escritura frenada por backpressure, la tarea espera dentro de una rama del `select` y tampoco atiende lectura. El actor puede seguir encolando mensajes. El cierre se coloca detrás de lo pendiente y no dispone de cancelación directa por sesión. R1 puede prolongar esa situación. No se midió el crecimiento ni se demuestra una fuga en toda llamada.

**Cambio:** límites de mensajes y bytes, deadline de escritura, cancelación y handle por sesión; reservar un camino de cierre que no dependa del backlog. Las órdenes de negociación/cifrado no deben descartarse silenciosamente.

**Aceptación:** un escritor sintético bloqueado conserva el presupuesto y finaliza dentro de un plazo definido al cerrar. La sesión siguiente no recibe eventos de la anterior. Un error de cola produce una transición explícita y observable.

### R3 P2 La cola N API puede perder transiciones críticas sin señalarlas

**Evidencia:** `host.rs:114`, capacidad 256; callbacks nativos, 343–346; `WsOpen`, 1060; `WsError`, 1095.

Esos envíos usan `NonBlocking` e ignoran el resultado. Con cola llena, Node-API devuelve `napi_queue_full` sin encolar el evento. Se pueden perder callbacks de negociación o errores, aunque existan otros deadlines. La consecuencia demostrada es pérdida silenciosa bajo saturación, no un bypass del cifrado. [Contrato de Node-API](https://nodejs.org/api/n-api.html#n_api_napi_call_threadsafe_function).

**Cambio:** tratar `QueueFull` y `Closing`; separar/reservar capacidad para control, coalescer telemetría y definir cierre seguro ante saturación. No cambiar a `Blocking` dentro de callbacks del hilo JavaScript: ese hilo también debe vaciar la cola.

**Aceptación:** saturación sintética produce entrega de control o cierre explícito, sin bloqueo y con contadores agregados. La posible retención del payload en el camino de rechazo de `napi 2.16.17` requiere una comprobación separada de ownership; no se incluye como fuga demostrada.

### R4 P2 El chat repite parsing para filas que no cambiaron

**Evidencia:** `crates/app/src/ui/chat.rs:352–418` recorre todas las filas retenidas y llama a `draw_message`; `ui/chat/markdown.rs:7–9`, `message_markdown`, vuelve a ejecutar `markdown::parse`.

Hay un límite global de mensajes, pero cada repintado repite análisis. GIFs, scroll y vídeo pueden amplificar el coste. El comentario del render explica que medir todas las alturas conserva la geometría real: saltarse filas sin resolver esa dependencia puede introducir regresiones.

**Cambio:** primero caché acotada del documento analizado por contenido/revisión, reutilizable entre vistas; separar análisis puro de tema, ancho y recursos de presentación. Incluir presupuesto de bytes y limpieza por cuenta. Evaluar virtualización sólo después de medir el coste restante y mantener alturas exactas, anclas y selección.

**Aceptación:** contenido sin cambios no vuelve a parsearse al repintar; edición invalida la entrada correcta; temas, tamaño de fuente, menciones y ancho se actualizan sin datos obsoletos. Scroll, selección, historial y altura de medios mantienen su comportamiento.

### R5 P2 El compositor duplica análisis y cuenta borradores sin limitar sus bytes

**Evidencia:** `ui/chat.rs:2080`, clonación del borrador; layouter, 2111–2116; `composer_text_height`, 3297–3306; `ui/mentions.rs:431,461–467`, `editor_job`; máximo de 1024 borradores en `chat.rs:1803`.

La predicción de altura y el editor reconstruyen contenido similar. No hay presupuesto agregado de texto ni un límite local generoso por borrador. El tope de reconocimiento de Markdown de 64 KiB en `core/src/markdown.rs:168–172` conserva el resto como texto plano: no limita todo lo que después se clona/maqueta. El límite de envío de Discord es un contrato distinto y está recibiendo cambios concurrentes.

**Cambio:** cachear análisis por revisión y reutilizar resultados compatibles; reducir copias completas para detectar cambios; definir límites de bytes por borrador y globales, separados del límite de envío. Rechazar exceso con explicación y conservar el texto anterior; no truncar silenciosamente ni eliminar borradores con contenido.

**Aceptación:** pegados grandes quedan acotados; se conservan Unicode, IME, menciones, deshacer y texto previo. Un cambio de cuenta elimina las nuevas cachés. El cálculo de altura sigue coincidiendo con el editor.

### R6 P2 El importador TXT lee todo el archivo antes de aplicar el límite

**Evidencia:** `ui/dialogs.rs:1783–1789`, botón «Importar enlaces»: `read_to_string` ocurre dentro del callback UI, antes de comprobar 256 KiB.

Un archivo grande elegido por el usuario se carga entero y puede bloquear la UI antes de rechazarse. Es una entrada local; no demuestra un ataque remoto.

**Cambio:** lectura limitada a 256 KiB + 1, metadatos como filtro inicial y trabajo fuera del hilo UI. Revalidar el resultado acotado; los metadatos solos no evitan que el archivo cambie durante la lectura. Mostrar error y conservar favoritos.

**Aceptación:** archivos grandes o crecientes se rechazan sin cargarlos íntegros; el resultado llega sólo a la operación vigente; no cambia la lista ante error.

### R7 P2 La comparación de snapshots recorre listas con el mismo almacenamiento

**Evidencia:** `core/src/shared_list.rs:97–100`, `PartialEq<Self>` compara los vectores; `app/src/ui/mod.rs:778` compara mensajes al recibir snapshots.

`SharedList` comparte el `Arc` y evita copiar elementos, pero la igualdad vuelve a recorrer contenido. La frecuencia real y el beneficio deben medirse.

**Cambio:** identidad/revisión explícita para el snapshot concreto, dentro del mismo ámbito de cuenta, canal, vista y generación, con comparación de contenido cuando corresponda. Una revisión coincidente entre ámbitos no demuestra igualdad. No cambiar genéricamente `PartialEq<T>` a `ptr_eq || contenido`: `PartialEq` puede no ser reflexiva, por ejemplo con NaN. Mantener su contrato o restringir una API nueva a tipos adecuados.

**Aceptación:** snapshots con almacenamiento/revisión idénticos evitan recorrer mensajes; edición, sustitución y nueva navegación siguen detectándose. La semántica genérica permanece intacta.

### R8 P3 El mantenimiento despierta a 50 Hz aunque parte del trabajo sea lento

**Evidencia:** `controller.rs:239`, intervalo de 20 ms; `Controller::tick`, 2329–2330, llama a `tick_game_activity`; `controller/game_activity.rs:158–162` reconstruye identidad y 204–234 construye y reemplaza notas de estado. El escaneo real ya está separado a cinco segundos.

**Cambio:** cachear identidad/notas según entradas relevantes y separar plazos. Mantener latencia de PTT, cierres, acknowledgements, expiraciones, seguridad y RPC; no limitar todo el controlador a cinco segundos. La elección entre scheduler por deadlines y una simplificación menor depende del perfil de CPU.

**Aceptación:** reposo sin necesidad de PTT/voz evita el ritmo rápido innecesario, y cada función conserva su plazo. No atribuir un porcentaje de CPU a este hallazgo sin medir.

### R9 P2 Privacidad local de las copias de sesión migradas

**Evidencia:** `app/src/shared_state.rs:114–123,206–208` respalda `session.dpapi`; 223–229 copia sesión y escribe `.session-migrated`; `storage.rs:183–189`, `forget_session`, elimina sólo la sesión activa.

Pueden quedar copias cifradas después del logout en respaldos o fuentes antiguas. El marcador evita reimportarlas automáticamente: no se ha demostrado un logout roto. El riesgo requiere acceso local bajo el mismo usuario Windows; DPAPI está asociado a esas credenciales, no a una única aplicación. [Ámbito de DPAPI documentado por Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata).

**Cambio:** inventariar recursos propios de credencial, evitar respaldos de sesión innecesarios y definir limpieza de los backups propios al olvidar sesión. Distinguir preferencias recuperables de credenciales. Conservar fuentes de Plus y copias ajenas; cualquier limpieza que las alcance necesita resolver expresamente ese alcance. No usar borrado recursivo de directorios de migración como sustituto.

**Aceptación:** fixture con sesión, backup propio y archivos ajenos elimina únicamente lo acordado, mantiene preferencias y no reimporta sesión. Los errores de borrado siguen siendo visibles. El borrado local no debe presentarse como revocación remota del token.

### R10 P2 La preparación de runtime decide algunas copias sólo por tamaño

**Evidencia:** `crates/media-addon/Prepare-Host.ps1:13–15,26–28` omite copiar si el destino existe y tiene igual longitud.

Dos motores diferentes del mismo tamaño pueden dejar un runtime antiguo o mezclado. Los hashes posteriores de `package.ps1` verifican lo copiado desde ese runtime, sin comprobar que corresponda al motor de referencia pretendido. Es un defecto reproducible del criterio de preparación; no evidencia de un motor manipulado instalado.

**Cambio:** comparar hashes o copiar siempre a un staging limpio por versión; inventario de motor, versión/ABI, procedencia y SHA-256 por componente. El empaquetado debe consumir ese inventario y detectar mezclas y archivos inesperados.

**Aceptación:** dos fixtures de igual tamaño y contenido distinto no se consideran idénticos. Una preparación fallida no convierte el staging parcial en una distribución válida; reproducir el inventario conserva los hashes esperados.

### R11 P3 Los límites de voz deben aplicarse antes de acumular el mensaje

**Evidencia:** `host.rs:1058` abre WebSocket con defaults; `media-addon/src/protocol.rs:3,202–223` rechaza mensajes por encima de 1 MiB después de recibirlos. Los defaults de Tungstenite 0.28 permiten mensajes mayores.

**Cambio:** configurar `max_message_size` y `max_frame_size` coherentes con el protocolo y sus mensajes legítimos. Verificar fragmentación válida. Evita reservar de más antes de rechazar; no se observó un ataque real.

**Aceptación:** el transporte rechaza por encima del límite antes del parser; los mensajes válidos fragmentados siguen aceptándose. No confundir este socket con Gateway principal, que ya dispone de otros presupuestos.

### R12 P3 Endurecimiento de distribución y retención de actualizaciones

**Evidencia:** `app/src/updates.rs:424–458,478–609` valida ruta, versión, tamaño y SHA-256; `scripts/install-update.ps1:38–44,167–200` vuelve a verificar antes de ejecutar. La publicación y sus hashes proceden del mismo repositorio. `package.ps1:26–28` copia ampliamente archivos del runtime; `build-installer.ps1` filtra rutas y carpetas de datos conocidas, sin inventario completo permitido. La preparación crea un directorio nuevo por intento y no establece una política de retirada del instalador descargado.

**Cambio:** mantener HTTPS y todas las validaciones actuales; considerar manifiestos firmados con clave de verificación incorporada y protección de la clave privada fuera del repositorio de publicación. Un hash desde el mismo origen comprueba integridad frente a esa publicación; no protege por sí solo de quien pueda reemplazar manifiesto y asset. La firma y su rotación requieren un diseño concreto, no una clave privada incluida en fuentes. Complementar con inventario permitido del runtime y rechazo de archivos inesperados. Añadir retirada segura de staging propio por estado/antigüedad/cuota, conservando la actualización activa y diagnósticos necesarios.

**Aceptación:** manifiesto/asset alterados sin firma válida no se instalan; no se publican datos sintéticos añadidos a una ruta inesperada del runtime. Intentos fallidos repetidos no acumulan instaladores indefinidamente. No borrar carpetas ajenas ni el staging con lease activo. Un árbol con reparse points se rechaza.

Esto es endurecimiento frente a compromiso de publicación y errores de empaquetado, no una vía remota demostrada contra un repositorio íntegro. La retención de instaladores afecta principalmente a disco, no explica por sí misma la fluctuación de RAM.

### R13 P3 Reducir copias temporales de secretos en memoria

**Evidencia:** `core/src/api.rs:123–128` copia token al encabezado; `Api::token`, aproximadamente línea 159, genera otra cadena para Gateway; `gateway_runtime.rs:616–618` lo incorpora a JSON y después lo serializa.

El original usa `Zeroizing`, pero su garantía no se extiende automáticamente a todas las copias. Reducir clones, centralizar serialización secreta y proteger temporales controlables limita exposición a volcados. No garantiza borrar buffers internos de HTTP/TLS/WebSocket y no demuestra una filtración. [Limitaciones de zeroize](https://docs.rs/zeroize/latest/zeroize/).

**Aceptación:** Debug y errores nunca incluyen credenciales; los temporales controlables tienen vida mínima y borrado donde corresponda. No generar dumps de cuentas reales para validar este punto.

## Protecciones que deben conservarse

| Área | Protección observada |
|---|---|
| Inicio de sesión | Credencial tipada, `Zeroizing`, Debug redactado, DPAPI, perfil WebView aislado, captura de API y navegación restringidas (`authentication.rs`, `auth_lifecycle.rs`) |
| API autenticada | Origen `https://discord.com:443/api/v9/`, encabezado sensible y redirecciones deshabilitadas (`core/src/api.rs`) |
| Cambio de sesión | Generaciones, cancelación, limpieza del controlador y recursos UI (`controller.rs`, `ui/mod.rs`, `Pictures::clear_account`) |
| Gateway principal | Colas y bytes acotados, timeout de escritura, backoff, cancelación y corte compartido de seguridad; no trasladar aquí automáticamente R1/R2 |
| Memoria de mensajes | Tres canales, 200 mensajes y 4 MiB estimados de texto/metadatos (`message_cache.rs:17`); no es el total de RAM del proceso |
| Imágenes y animaciones | LRU ordinaria de 8 MiB y 256 texturas; 512 usuarios de avatar; colas y generaciones; animaciones y FPS acotados (`ui/resources.rs`) |
| Vídeo e IPC | Último frame, presupuestos de streams/RGBA, pipe por usuario y PID, límites de tramas, Job Object y limpieza de entorno (`media.rs`, `core/src/video.rs`) |
| RPC de juegos | DACL por SID, rechazo de clientes remotos, comprobación de sesión/PID, límites de frames, clientes y frecuencia (`game_rpc/security.rs`, `protocol.rs`) |
| Render | Snapshot coalescido; miembros y servidores ya usan viewport. No proponer otra virtualización como si faltaran esas protecciones |

No se ejecutaron ataques, no se usaron credenciales, ni se inspeccionaron chats o almacenes reales. Una búsqueda heurística acotada en fuentes/configuración seleccionadas no mostró formatos obvios de tokens; no sustituye un escaneo completo del historial y artefactos. No se puede concluir que un sistema sea invulnerable a partir de revisión estática.

## Dependencias y motor externo

Se contrastaron avisos concretos de RustSec con `Cargo.lock`, sin actualizarlo:

- `rustls 0.23.46` está en el rango corregido de [RUSTSEC-2026-0348](https://rustsec.org/advisories/RUSTSEC-2026-0348.html) y [RUSTSEC-2026-0346](https://rustsec.org/advisories/RUSTSEC-2026-0346.html).
- `bytes 1.12.1` está en el rango corregido de [RUSTSEC-2026-0007](https://rustsec.org/advisories/RUSTSEC-2026-0007.html).
- No se ejecutó `cargo audit` ni se revisó exhaustivamente cada dependencia transitiva. No hay una certificación global de ausencia de avisos. En una implementación se debe ejecutar auditoría del lock con base identificada y revisar la alcanzabilidad por target/features, distinguiendo vulnerabilidades, avisos informativos y paquetes no usados en Windows.
- La versión de Electron y del motor nativo distribuido no queda identificada completamente por los manifiestos Rust. Inventariar el runtime real de la distribución, su procedencia, ABI y avisos antes de publicar; no ejecutar el motor para obtener esos datos si las restricciones lo impiden. [Recomendaciones oficiales de seguridad de Electron](https://www.electronjs.org/docs/latest/tutorial/security).

## Tandas de implementación y dependencias

| Tanda | Trabajo | Dependencias | Resultado verificable |
|---|---|---|---|
| T0 Medición y baseline | Fijar revisión/diff, identificar binario instalado frente a fuente; definir contadores y escenarios de RAM/CPU; resolver qué comprobaciones siguen permitidas | Ninguna | Baseline con mismas métricas, contenido y conjunto de procesos; datos de usuario separados de medición automática |
| T1 Voz y saturación | R1, R2, R3, R11; extraer política común de lifecycle/cola donde sea útil | Corte de fuentes T0; puede avanzar sin perfil UI | Timeout fiable, colas y tareas acotadas, cierre seguro, saturación visible |
| T2 Entradas y compositor | R5 y R6; separar análisis del texto de presentación, presupuestos y operación cancelable de importación | Baseline y contratos actuales de límites de mensajes | UI sin lectura entera ni texto ilimitado por un pegado; contenido previo conservado |
| T3 Render y mantenimiento | R4, R7 y optimización medida de R8; reutilizar el análisis de T2 | T2 para caché común; perfil antes de virtualización/scheduler amplio | Menos parsing, comparación y asignación con las mismas interacciones |
| T4 Credenciales y privacidad | R9 y R13; definir propiedad de backups y restauración sin revivir sesión olvidada | Inventario de migraciones y alcance de Plus | Sesiones propias tratadas según política explícita; errores visibles y sin borrado ajeno |
| T5 Distribución | R10 y R12; inventario/ABI, staging reproducible, datos excluidos, diseño de firma y retención | Diseño de claves/rotación si se adopta firma; no requiere tocar Plus | Runtime coherente y paquete con componentes permitidos, origen verificable y cuota de staging |
| T6 Integración y entrega | Revisión independiente, comprobaciones permitidas, comparación con baseline y cierre de hallazgos | Tandas incluidas en la entrega | Evidencia de aceptación por hallazgo; versión y publicación sólo según pedido vigente |

T1, T2 y la investigación de T4/T5 pueden dividirse entre agentes con propiedad explícita de archivos. No editar `host.rs` simultáneamente para R1/R2/R3. Integrar cambios antes de compilar. Evitar un refactor masivo de `controller.rs` por tamaño: extraer sólo contratos con responsabilidad clara que reduzcan duplicación demostrada.

## Criterios de validación y métricas

**Durante este análisis:** lectura estática de fuentes/scripts/manifiestos, contraste de documentación primaria y revisión paralela. No se ejecutaron benchmarks, pruebas funcionales ni compilación. El resultado no estima porcentajes de ahorro.

**Para una implementación autorizada:** escoger primero pruebas puras y sintéticas de los contratos anteriores, sin cuentas ni dispositivos. Verificar antes de ejecutar que no arranquen cliente, navegador, host multimedia, sockets externos, instalador o hardware; no ejecutar indiscriminadamente todos los probes. Las pruebas funcionales continúan pendientes mientras siga la restricción. Compilación, si corresponde al nuevo pedido, no acredita funcionamiento.

- Heartbeat con reloj controlado y escritor bloqueado; medir profundidad y bytes de cola, tareas restantes y plazo de cierre.
- Saturación del dispatcher con política explícita por clase de evento; sin silencios ni bloqueo del hilo que consume la cola.
- Caché de Markdown: análisis por revisión y coste de hit/miss, límite de bytes y liberación por cuenta. Validar geometría/selección cuando las pruebas UI estén permitidas.
- Importación acotada: archivo válido, grande, cambiante, ilegible y cancelado; sin lectura completa ni aplicación tardía.
- Migración/logout con credenciales sintéticas y archivos ajenos; nunca con sesión real ni dump.
- Runtime de igual tamaño/diferente contenido, archivo no permitido, firma inválida y staging activo; verificar fail-closed sin ejecutar Setup.
- Auditoría del lock, revisión de superficies `unsafe`/FFI modificadas y checks de Rust proporcionados a las tandas. No borrar dependencias por nombre sin estudiar features/targets y uso.

Comparar medianas y p95 de tiempo CPU/frame, CPU por segundo, private committed bytes y working set, con ventanas y condiciones iguales. Separar picos de decodificación de memoria retenida y memoria de GPU. Definir duración/número de ciclos antes de medir; no declarar fuga sólo porque el asignador no devuelve inmediatamente el pico al sistema. Los presupuestos internos de texturas/mensajes son parciales y no deben sumarse como si describieran toda la memoria del proceso.

## Datos, recuperación y entrega

Los cambios de caches y colas no deben requerir migrar historial ni volver a iniciar sesión. Si cambia el formato de preferencias, añadir esquema compatible y recuperación sin pérdida. Las credenciales no deben reaparecer al restaurar un backup genérico de ajustes. Mantener marcadores de verificación/corte de seguridad; logout o limpieza de backups no deben rebajarlos.

Las pruebas y el empaquetado usarán temporales propios con rutas comprobadas. Conservar el runtime anterior hasta verificar el nuevo staging. Para distribución se seguirá el orden existente de fuentes estabilizadas, compilación, paquete, instalador, verificación de hashes y publicación autorizada; no reconstruir un instalador con su propio hash dentro.

Entregable actual: este plan y la nueva skill de planificación. No implica publicar versión, instalar cliente ni cambiar la instalación del usuario. Entregables futuros: código por tandas, estado de cada hallazgo, comprobaciones realizadas y pendientes, métricas comparables y, sólo si se pide, instalador/publicación.

## Estado y pendientes

- [x] Revisar rendimiento, robustez, autenticación/API, privacidad, IPC y distribución en fuentes.
- [x] Incorporar la captura y el rango de RAM aportados por el usuario.
- [x] Separar fallos confirmados, endurecimiento e hipótesis de memoria.
- [x] Guardar el plan en el proyecto y someterlo a revisión independiente.
- [x] Crear después `notdiscord-planificar`, revisar sus escenarios y referencias, validar e instalar una copia idéntica en la carpeta personal de skills.
- [x] Implementar las tandas autorizadas; ver estado y límites de comprobación al final.
- [ ] Identificar mediante contadores el disparador de la bajada a 29–35 MB; distinguir foco, minimización, limpieza visual y recorte del sistema.
- [x] Auditar Cargo.lock y documentar identidad estática del runtime; avisos de mantenimiento y compatibilidad funcional siguen pendientes.
- [ ] Resolver política de credenciales heredadas que pertenezcan a Plus/otras copias antes de cualquier limpieza que las afecte.
- [ ] Si se adopta firma de manifiestos, definir custodia, rotación y recuperación de claves; no hay que inventar una clave de producción en esta tarea.
- [x] Revalidar el plan contra cambios concurrentes y versión vigente al empezar programación.

Huellas de referencia al corte: `Cargo.lock` SHA-256 `5c5bb9c64d79115be415b9f0403ddce01fcf821780d2ffe4f4911bbc176f68de`; `media-addon/src/host.rs` `ab6753dc0919f6ab3a05303590f17266ee470a13e6e4fefe0b3ef6ac6131cc87`; `core/src/shared_list.rs` `f6d6b9128cdb1ae63234a5e1db1afd3f933f971cbb4d54e7bbca0c56cdc964a4`. Los ficheros de UI/controlador reciben cambios concurrentes; no dar por vigente una línea antigua sin contrastar la función.

## Seguimiento de implementación autorizado — 0.5.0

Este apartado actualiza el estado del corte de análisis anterior. El usuario
pidió implementar el plan mediante notdiscord-programar e integrar la corrección
de límites pendiente. Eligió explícitamente Mediana, por lo que la versión de
entrega es 0.5.0 desde 0.4.1; prevalece sobre la propuesta Grande del análisis.
Después levantó la pausa de compilación/publicación: autorizó compilar y publicar
el instalador al terminar, sin ejecutarlo, usar su cuenta ni hacer pruebas
funcionales. NotDiscord+ permanece congelado.

| Hallazgo/tanda | Implementación y límite de verificación |
|---|---|
| T0 | Baseline de fuentes/artefacto y helper numérico optativo documentados en profile-baseline-0.5.0.md. No se ejecutó ni se midió una sesión; causa de la fluctuación de RAM y ahorros reales pendientes. |
| R1/R2/R11 | Heartbeat con plazo desde el primer envío sin ACK, vigilancia independiente del hilo JS, cola WebSocket 64 mensajes/4 MiB incluyendo el mensaje en escritura, timeout de escritura de 5 s, cancelación/AbortHandle por sesión y caps de 1 MiB antes del parser. |
| R3 | Payloads retenidos en mailbox Rust acotado (128 controles/64 MiB estimados), Tick/Mic coalescidos, wake NAPI sin payload y siempre NonBlocking. Saturación crítica cancela y cierra el helper si JS no atiende. La salida IPC original no se presenta como una cola acotada nueva. |
| R4 | AST compartido por contenido exacto y máscaras de menciones, 256 entradas/4 MiB presupuestados, independiente de tema/ancho; limpieza por cuenta/generación. Se conserva geometría completa, sin nueva virtualización. |
| R5 | Presupuesto local transaccional de 256 KiB por texto y 8 MiB agregados de borradores/metadatos, distinto del derecho remoto de 2000/4000. Rechazos conservan texto anterior y las interacciones; sin afirmar que esos bytes sean RAM total. |
| R6 | Importador TXT fuera de UI: un worker, cola de un trabajo y un resultado, descriptor limitado a 256 KiB + 1, cancelación por operación/cuenta/generación/picker y aplicación atómica de favoritos. |
| R7 | API same_storage sólo para SharedList<ChatMessage>, utilizada con cuenta/generación/canal/servidor/vista coincidentes. La igualdad genérica conserva su contrato. |
| R8 | Identidad RPC y notas se reconstruyen sólo al cambiar sus entradas. Mantenimiento de 100 ms sin trabajo sensible y 20 ms con voz/PTT, RPC activo, solicitudes/expiraciones/ACK/typing/snapshot pendientes; los eventos acortan el deadline sin posponer ticks vencidos. Escaneo de juegos a 5 s y plazos existentes conservados. |
| R9 | No se crean nuevos backups de sesión. Olvidar sesión conserva el marcador antireimportación y retira sólo sesión/temporales y copias identificadas propias. Fuentes Plus/legacy, preferencias y marcadores de seguridad intactos; backups históricos no atribuibles se conservan. No se ejecutó limpieza sobre sesiones reales. |
| R10 | Staging nuevo, copia comprobada por SHA-256, inventario permitido consumido por package/verify/build-installer y rechazo de mezclas/archivos inesperados/reparse points. El runtime preparado tiene 31 componentes y PE x64. REA/cadenas/exportaciones identifican declaraciones Electron 42.11.11, ABI clásico del host 146 y Node-API 6 del módulo, ligadas a hashes y documentadas en runtime-static-0.5.0.md; no prueban compatibilidad funcional ni revisión del motor de voz. |
| R12 | Retención para staging nuevo de propiedad explícita, allowlist plana y lease entre cliente/helper: 6 carpetas/2 GiB/7 días, sin borrar preparaciones activas ni adoptar carpetas antiguas o ajenas. Diseño de firma en update-signing-design.md; firma de producción pendiente de custodia, bootstrap y claves. No se crearon claves ni se anuncian manifiestos firmados. |
| R13 | API/Gateway comparten secreto Zeroizing, serializan temporales controlables con borrado y Debug redactado. Persiste una copia exigida por la frontera WebSocket y no se promete borrar buffers internos de TLS/HTTP/WebSocket. |
| Límites previos | Validación compartida de texto final al enviar/editar/reintentar, archivos por cuenta/destino y lote; confirmación de edición correlacionada conserva el borrador ante rechazo. |

### Dependencias y comprobaciones de esta implementación

Se preparó cargo-audit 0.22.2 en herramientas aisladas del proyecto, sin instalación
global. La base RustSec utilizada es el commit
3279c99911354c400e1d41e47c7d60e39258b147 (1378 avisos al consultar).
La primera auditoría detectó RUSTSEC-2026-0384 en Tokio 1.54.0: la aplicación
usa Interval y runtime multihilo, dentro de las precondiciones publicadas. Se
actualizó Tokio a 1.54.1 en el lock y se descargó esa revisión. Cargo conservó
las demás versiones, reajustando algunas referencias windows-sys entre versiones
ya presentes durante la resolución.

La repetición con el mismo snapshot devolvió 0 vulnerabilidades y 4 avisos de
mantenimiento: ttf-parser, zbus_names, zvariant y zvariant_derive. El árbol de
Windows x64 incluye ttf-parser; las otras tres no aparecen en ese target. Son
avisos informativos que permanecen registrados, sin sustituir fuentes tipográficas
o dependencias ajenas indiscriminadamente. La ejecución usó --no-yanked; no es
un escaneo completo de versiones retiradas ni una auditoría del motor nativo.

Las revisiones cruzadas no ejecutaron clientes, cuentas, hardware o pruebas.
La primera comprobación cargo check del componente multimedia pasó en 25,71 s;
la primera comprobación del workspace y todos los destinos pasó en 57,29 s.
Se repetirá lo necesario tras los ajustes de revisión y se registrará abajo el
resultado definitivo de compilación, paquete, instalador y publicación.

Referencia primaria del hallazgo añadido:
[RUSTSEC-2026-0384](https://rustsec.org/advisories/RUSTSEC-2026-0384.html).