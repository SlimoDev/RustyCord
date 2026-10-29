# Fuentes de llamada ya activas — 0.9.0

El usuario precisó que ocurre en la tarjeta propia y en las remotas, con otra persona conectada y cámara/pantalla ya transmitiendo. La causa pendiente de cifrado corregida en 0.8.0 no basta para explicar ese caso. Se continúa la corrección como actualización mediana, inferida por afectar al estado y restauración de fuentes activas. NotDiscord+ sigue congelado.

## Evidencia estática

- La galería normal recibe el snapshot completo y dibuja ambos controles de forma independiente. Seleccionar uno cambia la imagen mostrada y el texto del botón, sin suprimir el otro. El modo ampliado omite la galería deliberadamente; por sí solo no explica el síntoma del último medio.
- `ready_metadata` convertía `self_video` y `self_stream` ausentes en booleanos falsos. `incoming_participant` los interpretaba como retiradas explícitas y el helper de disponibilidad impedía usar el fallback nativo. Se perdía la diferencia entre ausencia de anuncio y anuncio negativo antes de llegar al controlador.
- Para participantes sin anuncio explícito, la disponibilidad sólo podía recuperarse desde `CallVideo`. Un descriptor pertenece a la recepción: puede desaparecer al ocultar, seleccionar otra fuente o navegar. Al desaparecer, no quedaba una oferta estable. Los callbacks positivos de cámara se ignoraban junto con los negativos desde 0.7.0.
- `cameraState` recalculaba siempre `camera_enabled` como `voice_encryption_ready && active`. Una cámara previamente confirmada podía quedar falsa por un callback durante una transición de cifrado; volver a estado conectado no reconstruía ese booleano. `shareState` también convertía estados de transporte transitorios en `screen_sharing=false`. Los fotogramas, los estados de transporte y los anuncios de publicación llegan por recorridos distintos.

Estas rutas son defectos alcanzables de las fuentes, no una captura de la sesión del usuario. No se accede a la cuenta, al cliente en ejecución, a memoria, mensajes ni dispositivos para comprobar el incidente.

## Contratos

- Los campos ausentes/nulos/malformados conservan su ausencia; un `false` o `true` real conserva su presencia.
- Una observación positiva autorizada de cámara/pantalla remota puede conservar una oferta desconocida en el roster de la llamada, por tipo. El descriptor y la textura pueden desaparecer sin borrar esa oferta. Un anuncio explícito negativo conserva prioridad; un callback tardío no lo revierte. Membresía, salida, roster completo y nuevo intento siguen limitando su vida.
- Un delta de roster que omite un tipo no retira la información previa de ese tipo. No se transforma el último medio recibido en la totalidad de las fuentes del participante.
- La cámara y la pantalla propias ya confirmadas conservan su estado mientras el transporte renegocia. Una fuente nueva continúa esperando cifrado y confirmación; stop/error/cierre de llamada siguen retirando la publicación.
- El ACK de cámara también conserva una activación ya confirmada si el cifrado cambia antes de llegar la respuesta. Un callback terminal aceptado invalida el intento de cámara; su ACK y callbacks antiguos no pueden reactivarlo. El siguiente inicio explícito usa un token nuevo.
- El diagnóstico añade únicamente estados y conteos numéricos cuando cambian, en el controlador y en la interfaz. No guarda identificadores, nombres, direcciones, dispositivos, mensajes ni payloads.

## Lectura de diagnóstico

`CallSourcesState`, componente `Voice`/fase `Dispatch`, resume el controlador. El componente `App`/fase `Present` resume la interfaz y su visibilidad. `correlation` es una máscara numérica, nunca un ID:

| Bits | Contenido |
|---|---|
| 0, 1 | Cámara propia, pantalla propia |
| 2, 3 | Preparación propia de cámara, pantalla |
| 4, 5 | Cifrado listo, voz conectada |
| 6, 7 | Anuncio propio conocido de cámara, pantalla |
| 8–15, 16–23, 24–31 | Cantidad de remotos con cámara, pantalla, ambas |
| 32–39, 40–47 | Remotos cuyo anuncio de cámara/pantalla es desconocido |
| 48–55 | Cantidad de descriptores de vídeo |
| 56–62 | Cantidad de participantes |
| 63 | Panel de llamada visible, sólo en el registro de interfaz |

Los conteos se saturan; no se asignan números persistentes a los usuarios. La deduplicación evita registrar cada fotograma. Estos registros permiten separar pérdida de ofertas, retirada explícita y vida del receptor si el síntoma persiste.

## Comprobaciones y límites

Revisión estática y regresiones añadidas como fuente; no se ejecutan pruebas funcionales, sintéticas ni con dispositivos por la restricción vigente. Compilación y entrega se registrarán al finalizar. El comportamiento real permanece pendiente de confirmación del usuario. Sin una retirada explícita, una oferta observada y desconocida se conserva hasta salir o renovarse la llamada; el fin de un decoder no demuestra por sí mismo el fin de la publicación.

- Revisión independiente final: sin hallazgos bloqueantes, incluida la carrera de ACK y los límites de privacidad de diagnóstico.
- `git diff --check`: correcto.
- `cargo check --workspace --all-targets --release --offline --locked -j1`: correcto, 41,94 segundos; compila las regresiones sin ejecutarlas. Conserva las advertencias previas fuera de este cambio.
- Runtime preparado y contrastado: 31 componentes; no se ejecuta.
- Instalador y compilación de producción: pendientes de registrar tras su generación.
