# Mensajes y panel de usuario en 0.4.0

Entrega **Mediana**, por elección explícita del usuario, desde 0.3.2 a 0.4.0.
Agrupa el control de actualización y las mejoras de mensajes bajo la skill
`notdiscord-programar`, cuyo código se mantiene en `skills/notdiscord-programar`
en la raíz del repositorio. La skill de planificación continúa pendiente.

## Panel de usuario

El panel pintaba siempre el separador, Actualizar y la consulta manual; ambas
listas laterales reservaban además 58 puntos aunque no hubiera novedades.
`UpdateManager::control_visible` comparte ahora una sola condición entre el
dibujo, la protección del control y las dos reservas: debe haberse confirmado
una versión superior con un instalador válido.

Sin esa actualización se retiran la fila completa y su espacio. La consulta
manual sigue disponible en el menú del usuario como «Buscar actualizaciones».
Con una actualización confirmada se conservan progreso, preparación y reintento;
cuando otra consulta confirma que no hay actualización, el panel vuelve al modo
compacto. La consulta automática existente permanece independiente del dibujo.

## Enlaces y nombres

Los enlaces nativos de egui ya tenían subrayado al pasar el puntero y cursor de
mano; entre ellos está el aviso que abre el límite de descarga de imágenes/GIFs.
Los textos dibujados a medida y algunos labels que abren perfiles no compartían
esa presentación. Los helpers de feedback de texto añaden subrayado y mano al
hover, y subrayado al foco, manteniendo las respuestas y acciones originales.
El texto envuelto utiliza las filas de su galley, sin subrayar el ancho vacío de
una tarjeta ni crear una segunda superficie que capture selección o arrastre.

Se aplica a los nombres y enlaces personalizados de mensajes, respuestas,
menciones navegables y personas de reacciones, y a los textos de navegación que
usan los componentes de listas. `@everyone` y `@here` no se convierten en enlaces
a un perfil inexistente. Los botones y textos sin destino conservan su función.

`RichText::strong` de egui modifica el color, sin seleccionar por sí solo una
fuente de mayor peso. Los autores de los mensajes usan ahora una familia con
Segoe UI Bold, conservando tamaño y los fallbacks proporcionales para nombres
de otros alfabetos. La fuente se mapea una vez y los fallbacks se actualizan
cuando se cargan alfabetos adicionales. Si Windows no proporciona esa fuente,
se conserva la familia disponible para no perder nombres ni glifos.

## Menús de enlaces y mensajes

El menú general de mensaje ya disponía de responder, reenviar, reaccionar,
fijar/desfijar, editar y eliminar cuando el contexto y los permisos lo permiten.
Los enlaces y medios tienen sus propias superficies interactivas; por eso no
bastaba con añadir opciones al fondo de la fila para que aparecieran sobre ellos.

Se compone un único menú contextual por mensaje. El contenido aporta las
opciones de su destino y el menú reutiliza las acciones generales existentes.
Los enlaces HTTP(S) ofrecen abrir y copiar el enlace real; los medios conservan
sus opciones específicas. El clic derecho abre el menú, sin ejecutar el destino
ni activar reproducción o ampliación. Las acciones de escritura mantienen las
validaciones y el borrado sigue utilizando su confirmación.

Las vistas de búsqueda, fijados, historial, mensajes reenviados y eliminados
conservan sus restricciones. Añadir un menú a una vista previa eliminada no
habilita volver a descargar o reproducir el contenido.

## Markdown en compositor y mensajes

El cuerpo se interpretaba principalmente como texto con menciones y enlaces;
los delimitadores de formato seguían visibles en el mensaje enviado. Ahora un
parser compartido conserva rangos UTF-8 del original y separa bloques, contenido
y sintaxis. El compositor utiliza esos rangos para mostrar el formato mientras
mantiene todos los caracteres editables y sus delimitadores atenuados. El envío
conserva el contenido original; la presentación del mensaje oculta la sintaxis.

Se presentan negrita, cursiva, subrayado, tachado, spoilers, enlaces enmascarados,
código, encabezados, subtexto, citas y listas ordenadas o sin numerar. La fuente
usa variantes reales Bold, Italic y Bold Italic cuando Windows las proporciona,
con los fallbacks de alfabetos existentes. El código conserva su contenido
literal y no activa menciones, enlaces ni multimedia. El autocompletado no
convierte un `@Nombre` protegido por código o escape en una mención al enviar.

La interpretación tiene límites de longitud, bloques, spans, profundidad y
trabajo; el resto se conserva literal. El alcance es compositor y cuerpo de
mensaje, manteniendo las reglas anteriores de bios y títulos de embeds.

La [evidencia de REA](rea-markdown-0.4.0.md) documenta las reglas recuperadas del
asset público, su identidad, límites de análisis y decisiones de implementación.
La captura del usuario fundamenta el caso de negrita con espacio antes del cierre
(`**texto **`); la regla heredada de negrita de Discord no se recuperó con REA.
No se afirma equivalencia completa con Discord ni funcionamiento comprobado.

## Eliminación de registros locales

Un registro de mensaje ya eliminado se puede retirar mediante la acción de
eliminar del hover, menú contextual y atajos existentes. La acción compartida
`purge-deleted-log` valida que sea un registro eliminado, retira sus snapshots,
caché y contenido retenido, y bloquea su reconstrucción por respuestas tardías
durante la sesión. No envía una segunda eliminación a Discord. Si se alcanza el
límite de marcadores, rechaza la acción antes de descartar datos parcialmente.

El ACK llega tras limpiar el estado y libera la vista previa multimedia local.
Las respuestas retenidas en borradores conservan sólo sus identificadores y el
estado no disponible; el texto escrito por el usuario permanece intacto. Los
registros son de esta sesión y residen en RAM. Esto no promete borrar copias
externas ni realizar un borrado forense del equipo.

## Seguimiento del historial

Enviar o reintentar un mensaje utiliza la misma acción que «Volver al inicio»:
recupera la página reciente si hace falta y se desplaza al mensaje más nuevo.
El ACK sólo afecta al canal que continúa abierto. Una solicitud pendiente del
canal anterior se descarta si el usuario navegó a otra vista o conversación.

Para mensajes entrantes, la lectura se registra por filas reales y sigue los
nuevos mensajes cuando quedan como máximo 20 después del último visible. Una
imagen alta y un mensaje de una línea cuentan como un mensaje cada uno. La
ventana antigua no se confunde con la página reciente; se mantienen las
protecciones de selección, desplazamiento manual y foco de navegación.

Los controles se dibujan únicamente mientras se pueden utilizar, con «Volver al
inicio» a la izquierda de «Mensajes anteriores». El primero desaparece al llegar
al extremo reciente; el segundo sólo aparece al poder cargar la página anterior
desde el extremo superior, sin otra carga o restauración pendiente.

## Comprobaciones y alcance

Se revisaron las fuentes y pasaron `cargo check --workspace --all-targets
--release --offline --locked -j1` (41,21 s) y la compilación de producción
`cargo build --workspace --release --offline --locked -j1` (8 min 54 s).
El paquete de 39 archivos pasó `verify-package.ps1` y se compiló el instalador
con Inno Setup. Los 151 archivos Rust conservaron sus hashes durante la
compilación y el empaquetado. La skill pasó el validador oficial, una evaluación
independiente en seco de su clasificación y límites, y la comparación entre
sus cuatro archivos instalados y los del repositorio.

| Artefacto | Identidad verificada |
|---|---|
| `NotDiscord-Setup-0.4.0.exe` | 118 408 751 bytes; SHA-256 `c28100f98128ddccbd2f132816bc1b439da4a846e9da8386d827f557d82f9d7c` |
| `NotDiscord.exe` del paquete | SHA-256 `7d85d0ea3bfcafc3c64a39846fcd67f7d4664b45d46bd43bf7370969c399b0b5` |
| `NotDiscord.Media.node` | SHA-256 `936db4bb5aefc990401296b86f167fa49c47ac326b5b5d886371dd9e76693a31`, igual a la entrega anterior |

La [release 0.4.0](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.4.0)
contiene el instalador con estado `uploaded`, tamaño y digest SHA-256 remoto
coincidentes. El manifiesto que lo anuncia se publicó después de esa comparación;
el instalador y el ejecutable se compilaron previamente con `release: null` para
evitar incluir el hash de Setup dentro de sí mismo.

No se ejecutaron cliente,
instalador, dispositivos, cuenta o pruebas funcionales para esta entrega.
La presentación y funcionamiento en la aplicación quedan pendientes de
confirmación del usuario. NotDiscord+ mantiene su rama y artefactos anteriores.
