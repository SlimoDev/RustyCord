# Entrada y autoscroll de NotDiscord 0.7.0

Fecha: 28 de octubre de 2026, America/Lima. C01 y C06 de la continuación del
plan de interacción. NotDiscord+ se conserva. Cambios revisados en fuentes;
no se inició la aplicación ni se leyó el portapapeles real. No se ejecutaron
pruebas funcionales ni sintéticas. La compilación de integración corresponde
al cierre de la tanda y no demuestra gestos físicos de Windows.

## Causas y contrato central

El controlador anterior esperaba `button_clicked(Middle)`, un release que
egui reconoce como clic y que caduca al mantener o mover. Además vetaba todo
`Sense::click`, incluidas filas primarias completas. Cancelaba por la cola de
suavizado de una rueda anterior. El pan genérico de ScrollArea competía con el
controlador, y el historial no exigía overflow real para armarlo.

`ui/autoscroll.rs` procesa ahora down, movimientos y up nativos ordenados,
aunque lleguen en el mismo lote. Un down elegible fija el área interior, capa,
viewport, origen y ejes con overflow real. Mantener y salir de la zona muerta
pasa de Initial a Holding; el up lo termina. Un up sin desplazamiento efectivo
pasa a Toggled; el movimiento posterior mantiene velocidad incluso si el
puntero se queda inmóvil. La duración del hold no depende del deadline de clic
de egui. Un nuevo down central, Escape, una rueda nueva, foco perdido,
desaparición de ámbito o un popup superior lo retiran.

Los paneles ordinarios del shell también bloquean el gesto primario/secundario
de cancelación hasta su release, sin abrir un modal ni activar una fila o botón
que estuviera debajo. Los nuevos átomos de texto conservan elegibilidad central.

La rueda se identifica por `Event::MouseWheel`, no por `smooth_scroll_delta`
residual. Mientras el gesto central está activo, el wrapper suspende sólo el
pan competidor y el suavizado viejo; arrastre primario/touch y rueda nueva
conservan su recorrido. El historial conserva sus anclajes, selección,
paginación y propietario de rueda. No se duplicó su avance en el wrapper.
Sólo se avanza en la primera pasada del frame y se repinta a 16 ms mientras
existe un dueño; inactivo no mantiene ese temporizador. Los wrappers verticales
y horizontales usan ejes explícitos y límites del contenido.

Las filas y botones de clic primario ya no se excluyen automáticamente. Los
sliders/editores y superficies con arrastre o reservas multimedia mantienen sus
gestos. El rail de servidores declara explícitamente que su reorder es sólo
primario, sin achicar su hitbox. Un gesto primario/secundario que cancela el
modo deshabilita temporalmente los hijos de las áreas desplazables hasta su
release para impedir navegación accidental. El historial conserva además su
barrera de cancelación compartida. El estado no vuelve a entrar a Context
mientras conserva una transacción de datos.

No se cambió winit para ratón ni se añadió hook. El patrón de estados sigue la
referencia Chromium identificada en el plan; no se afirma equivalencia numérica
observada de velocidad, zona muerta o cursor con una versión concreta de Discord.

## Primer pegado de bitmap

`egui-winit` consumía la pulsación Ctrl+V/Shift+Insert y sólo emitía `Paste` si
obtenía texto no vacío. Un bitmap sin texto no entregaba al dueño de campos
ninguna intención. El posterior polling del compositor necesitaba que ya
tuviera foco y podía perder la transición de una pulsación rápida.

El adaptador conserva ahora el evento real de tecla, su orden, modificadores,
tecla lógica/física y repeat, únicamente cuando no hay representación textual.
Con texto se conserva exactamente el Paste textual anterior. No hay texto
ficticio ni IME sintético. `input_ownership` recibe la intención antes de que el
TextEdit procese eventos y describe capacidad Text/Attachments. Sólo el
compositor enfocado y permitido, sin overlay/popup/visor u otro dueño, puede
reclamar bitmap. La imagen no se consulta desde búsquedas, apodos, formularios,
editor de mensajes o el fondo de un modal. Se consume una sola pulsación
no repetida; un gesto posterior vuelve a ser válido. Se elimina el polling de
teclas de la ruta del compositor.

Texto no vacío tiene prioridad cuando el clipboard tiene formatos mixtos.
La acción explícita Pegar imagen sigue permitiendo elegir bitmap. Preparar PNG
continúa en el worker de Pictures, con sus colas acotadas. Solicitud y resultado
llevan cuenta, generación, canal y operación efímera; un registro de hasta ocho
operaciones reclama cada respuesta una sola vez. Un resultado de otra cuenta,
generación, canal actual o destino sin permiso se descarta y su propietario
elimina el temporal; no crea un adjunto en el campo de detrás. Errores de cola
retiran la operación pendiente y permiten otro intento. No se guardan eventos
ni contenido del portapapeles en logs.

## Comprobación pendiente del usuario

- Central: down/up juntos; hold superior a 0,8 s; mantener y mover, luego dejar
  quieto; salir del área; volver; segunda pulsación; Escape; foco/cambio de
  vista/popup; rueda anterior frente a rueda nueva; área anidada y sus límites.
- Áreas: historial, rail, canales/DM, miembros, GIFs y diálogos. Primario sigue
  navegando/reordenando y seleccionando; los sliders y el visor conservan drag.
- Clipboard: primer bitmap sin foco previo, gesto rápido, mantener, segunda
  pulsación, Ctrl+V/Shift+Insert, texto, mixto y vacío. Búsqueda/modal/visor y
  falta de permisos no adjuntan detrás; navegación/logout durante preparación
  no aplican una respuesta desactualizada.

Una fixture de `Text` o de clic ya completado no demuestra el recorrido nativo
Windows → egui → Owner → acción/offset. No se presenta como probado por compilar.
