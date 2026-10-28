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
- `cargo build --workspace --release --offline --locked -j1`: correcto, 8 minutos y 2 segundos. Identidad de compilación `e575c85f19902e06f08bbe901a466fc81405ac89`; 425 fuentes, inventario SHA-256 `7dc3bf454f028062e62e44ba9bce37b1b05e30123f4c791cd5b93c594a9953ce`, contrastado de nuevo antes del empaquetado.
- Paquete temporal: 41 archivos, `verify-package.ps1` correcto. El complemento nativo conserva el hash de 0.7.0: `0ab369b57973d1837b68b34881f3168e1a68b2a94e7d3441c7a1607131cc75d5`.
- Instalador generado con Inno Setup y publicado como [NotDiscord 0.8.0](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.8.0). Un único asset `NotDiscord-Setup-0.8.0.exe`, 118786679 bytes; SHA-256 local y digest remoto coinciden: `0c4f79aeeea2eaf20c03af4119ed57038c6b0830ebedbb82132884fe56bc5fa0`.
- Ejecutable incluido: SHA-256 `c41c67da7a22cbf4cd1564828e3c31578f8ebd0741d6ca247ea6ec76fc99e202`. El manifiesto de actualización anuncia este instalador después de verificar el asset remoto. No se reconstruye el mismo Setup con su propio hash dentro.
- Instalación y accesos directos del equipo conservados; no se ejecutó el instalador ni el cliente.
- Edición Plus conservada: sus 38 archivos mantienen el inventario SHA-256 `FEFB209E0B35C77498DEFC90169B7BBC7519E32BE9C449AFFFDF8ABFBC11C605` y su rama sigue en `d35c06ac7e824c17198791b3d9208f732bc8be81`. `legacy` permanece en `637d41f897a0e435c3d7e26d1c400afd6a640d0e`.
- Funcionamiento en llamada, navegación y dispositivos: pendiente de confirmación del usuario por su restricción de pruebas funcionales.
