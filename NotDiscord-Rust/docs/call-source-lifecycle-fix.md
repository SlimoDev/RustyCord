# Ciclo de vida de cámara y pantalla locales — 0.8.0

Actualización mediana, inferida según la skill de programación: corrige un defecto de estado y restauración de fuentes de llamada. Continúa la entrega 0.7.0 sin modificar NotDiscord+.

## Causa demostrada en 0.7.0

Antes de disponer de cifrado, seleccionar una cámara y una pantalla conserva dos intenciones de publicación independientes (`pending_camera` y `pending_share`). Ambas quedan en preparación y todavía no se publican. Sin embargo, los botones de la cuadrícula dependían del descriptor de la vista previa física. El motor sólo conserva una vista previa: la segunda selección sustituía a la primera. Al ocultarse el área de llamada, se liberaba también la última vista previa; al regresar ya no había descriptor del que inferir ninguno de los botones.

`reset_navigation` no borra los flags de llamada. La pérdida ocurre al confundir la oferta local pendiente con la vida del único sink de previsualización. La inspección no demostró ese mismo defecto para dos publicaciones ya activas y cifradas.

## Corrección

- El modelo distingue preparación local y publicación activa. Sólo el participante propio puede ofrecer una fuente pendiente; esto no anuncia vídeo en el Gateway ni evita el requisito de cifrado.
- La elección local se conserva en `ViewIntent`, con el intento de llamada. Mostrar una fuente pendiente recrea la vista previa desde el dispositivo o pantalla ya seleccionados. Alternar cámara/pantalla cambia la vista previa física, conservando las dos ofertas y las dos selecciones de publicación.
- Ocultar el panel libera el preview propio y conserva oferta e intención. Volver al mismo panel sólo rearma la elección vigente. Un selector modal tiene prioridad y no reactiva por sí mismo la vista previa de una llamada que está en otro canal.
- La visibilidad del host se confirma de forma asíncrona. Sólo existe una orden de visibilidad en vuelo; el ACK habilita el siguiente estado deseado y, finalmente, el rearme. El actor sigue procesando voz y acciones mientras espera. Cambiar únicamente la visibilidad del panel no repite la orden de visibilidad global.
- Cerrar una visualización no cancela la otra oferta ni la publicación. Los controles de cancelar cámara/pantalla eliminan únicamente su fuente e intención correspondientes. Salir de voz limpia todas las intenciones del intento terminado.
- Al obtener cifrado se activan ambas selecciones explícitas aún vigentes. Los nuevos sinks heredan la elección del mismo tipo; una cámara que llega primero no sustituye la pantalla seleccionada. La voz se conserva.
- Un fallo o un preview que no alcanza el estado activo en 35 segundos presenta error y permite reintentar explícitamente; el mantenimiento no genera un bucle de aperturas. Ese plazo corresponde al preview local solicitado, no a la negociación de cifrado. El plazo de vigencia de la selección pendiente ya existente (90 segundos) también se evalúa durante el mantenimiento, incluso si el cifrado nunca llega.

## Evidencia y alcance de comprobación

Se añadieron regresiones como código fuente para ambos órdenes de selección, navegación y ACK cruzados, eventos antiguos `paused`/`removed`, cancelación independiente, prioridad del selector, reintento explícito y promoción de las dos fuentes al quedar listo el cifrado. Utilizan puentes sintéticos y un registrador de Gateway sin conexión.

Estas pruebas no se ejecutaron. No se abrió el cliente, no se accedió a ninguna cuenta y no se realizaron pruebas funcionales ni pruebas con cámara, micrófono o pantalla. La compilación y publicación se gestionan por separado en la entrega principal.

## Comprobaciones de entrega

- `git diff --check`: correcto.
- `cargo check --workspace --all-targets --release --offline --locked -j1`: correcto; última ejecución 26,07 segundos tras conservar el límite compartido de cuatro visualizaciones. Comprueba también que las regresiones fuente compilan; no las ejecuta. El compilador mantiene advertencias previas de código no utilizado y compatibilidad futura fuera de esta corrección.
- Revisión independiente del contrato UI/actor, ACK y tokens: sin hallazgos bloqueantes; tampoco ejecutó el producto.
- Runtime de referencia copiado y contrastado por inventario: 31 componentes, sin ejecutarlos.
- Binario e instalador de producción: pendientes de generar al registrar estas fuentes.
- Funcionamiento en llamada, navegación y dispositivos: pendiente de confirmación del usuario por su restricción de pruebas funcionales.
