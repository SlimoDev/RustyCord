# Interacción, desplazamiento y datos compartidos

Cambios revisados estáticamente. No se han iniciado la aplicación, una cuenta,
llamadas, dispositivos ni pruebas funcionales para verificarlos. Las posiciones,
el foco y los gestos quedan pendientes de confirmación por el usuario.

Esta entrega se aplica únicamente a **NotDiscord**. Por indicación posterior del
usuario, NotDiscord+ conserva su rama, ejecutable y acceso directo anteriores.
El diseño compatible de datos y exclusión queda preparado para una futura
adopción por Plus; no se afirma que el binario de Plus conservado lo utilice.

## Llamadas, superposiciones y mensajes

La notificación entrante retenía la acción de un botón en una variable que una
pasada posterior de dibujo podía volver a dejar vacía. Ahora se conserva la
elección entre pasadas, se bloquea el doble envío mientras se procesa y se
concede al proceso de conexión el margen necesario. La aceptación detiene el
temporizador de rechazo antes de esperar la salida de una llamada anterior.
Se reutiliza la operación real de conexión, su validación de contexto y sus
estados de error; no se simula una conexión desde la ventana del aviso.

`ui/overlay.rs` centraliza clic exterior y Escape para los diálogos, búsqueda,
selector y visor. Se considera la superficie hija superior y se mantienen los
popups de egui como ComboBox/menús. La pulsación que cierra bloquea el fondo ese
fotograma: cerrar no envía, acepta ni rechaza implícitamente. Cancelar ajustes
por clic exterior restaura su copia, mientras que un cierre ordenado de cambio
de instancia guarda los cambios según lo solicitado.

El selector de emojis conserva su apertura después de insertar Unicode o
emojis personalizados, respeta selección/caret y actualiza los metadatos de
menciones. GIFs y reacciones conservan su cierre contextual. El envío ya no
sustituye emojis inaccesibles por enlaces CDN generados: conserva tokens válidos
y rechaza el envío si no puede confirmar su uso, manteniendo el borrador. Los
enlaces escritos por el usuario no se ocultan. Los mensajes históricos que ya
contienen enlaces no se reescriben ni se confunden con el contenido nuevo.

El cursor de texto se aplica al texto seleccionable; enlaces conservan su mano
y los átomos gráficos de emoji mantienen su selección lógica sin forzar ese
cursor al pasar por ellos. Backspace mantenido y un clic en un mensaje propio
usa el mismo comando `delete` del menú sin diálogo de confirmación. Se excluyen
editores enfocados, selección de contenido, menús, reacciones, enlaces y
controles hijos. La propiedad de respuesta evita que el clic llegue a otro
mensaje; el controlador verifica autor, canal y permisos antes de la solicitud.
El atajo anterior Supr con confirmación y los dobles clics se conservan.

## Desplazamiento con el botón central

La interfaz utiliza `egui::ScrollArea`, que pinta sobre la ventana de la
aplicación y no crea un control de scroll nativo de Windows. Por ello se utiliza
un controlador compartido en `ui/autoscroll.rs`, conectado mediante los helpers
de `ui/theme.rs` y el contenedor común de diálogos.

La rueda pulsada inicia el modo en un espacio libre que realmente necesita
desplazamiento vertical. Se conserva el identificador del contenedor y el punto
de origen, incluso al mover el puntero a otra lista o fuera de sus límites.
Hay una zona de reposo vertical de 12 puntos; fuera de ella aumenta la velocidad
con la distancia, hasta un límite. Otra pulsación central o Escape lo cancela;
también se retira al perder el foco o desaparecer su contenedor. Cada viewport
mantiene su propio estado. El movimiento se aplica una sola vez por fotograma,
y una rueda explícita tiene prioridad ese fotograma.

Los contenedores anidados registran sus fondos de arrastre por separado de los
controles. El área interior elegible se evalúa primero; iniciar un modo no mueve
también su padre. El historial utiliza el mismo controlador, pero conserva su
gestión propia del desplazamiento por selección, rueda, anclajes y paginación;
su wrapper desactiva el segundo movimiento automático para evitar duplicarlo.

### Inventario de áreas

| Área evaluada | Tratamiento |
|---|---|
| Historial de chat de MD, grupos y servidores | Habilitado por el controlador compartido; conserva selección y anclajes del historial. |
| Mensajes fijados, resultados de búsqueda y registro de cambios de la sesión | Mismo recorrido de historial y mismos límites de interacción. |
| Carril de servidores y lista interior de servidores | Habilitado en sus espacios libres; sus iconos y filas que navegan quedan excluidos. |
| Lista de canales, categorías y conversaciones | Habilitado en el fondo; los destinos de navegación y controles mantienen el clic. |
| Miembros de servidor y de conversaciones | Habilitado en el fondo; nombres, avatares, botones y acciones mantienen su propiedad. |
| Amigos | Habilitado en los espacios libres; las filas y acciones conservan su interacción. |
| Perfil completo y panel lateral de perfil | Habilitado donde hay contenido vertical; banners, avatares y acciones quedan excluidos como origen del modo. |
| Cuadrícula de participantes y listas de llamadas entrantes | Habilitado cuando exceden la altura; las tarjetas multimedia, avatares y controles conservan sus gestos. |
| Ventana independiente de llamada entrante | Mismo controlador, con ciclo de vida por viewport; aceptar, ignorar y rechazar siguen siendo botones independientes. |
| Ajustes de interfaz y sonidos de eventos | Habilitado en el fondo de sus secciones; sliders, editores, selectores y botones quedan excluidos. |
| Voz, vídeo y dispositivos | Scroll exterior del diálogo mediante el helper común; la selección de texto y los controles mantienen prioridad. |
| Ajustes de servidor | Habilitado en `guild-editor-scroll`, sin capturar controles. |
| Listas de fuentes de pantalla y de cámaras | Habilitado en el fondo de la lista; las fuentes seleccionables no inician el modo. |
| Filtros y sugerencias de búsqueda | Habilitado en el fondo de sugerencias; entradas de filtro, teclado y editor conservan el foco. |
| Selector de GIFs y emojis Unicode/de servidor | Habilitado en espacios libres; las miniaturas y los emojis no inician el modo. |
| Panel de personas de una reacción y detalles de error | Habilitado si necesitan scroll; personas, reintentos y demás botones quedan excluidos. |
| Diálogos de miembros, añadir participantes, reenviar, edición y otros diálogos centrados | Scroll vertical común; campos editables, filas y acciones quedan excluidos. |
| Tiras horizontales de adjuntos y previsualización del borrador | Excluidas del modo vertical: conservan rueda/arrastre horizontal y controles de quitar/ampliar. |
| Visor de imagen y pantalla ampliada | El contenido conserva ampliación y desplazamiento por arrastre; no se inicia autoscroll sobre la imagen. El contenedor sólo admite el modo en espacios libres elegibles. |
| Historiales y listas vacíos o sin desbordamiento | No activan un modo sin recorrido disponible. |
| Editores multilínea, selección de texto, barras de desplazamiento y menús nativos de egui | Conservan su interacción propia; no se reemplaza la edición, la selección ni el arrastre de la barra. |

Los enlaces, botones, sliders y filas con acciones quedan excluidos utilizando
sus respuestas interactivas completas. Los emojis y los medios decorativos
con `Sense::hover` registran además sus rectángulos pintados explícitos: no se
confunden con un espacio libre por carecer de una acción de clic. Los labels de
texto seleccionable del historial son una excepción deliberada al veto de
controles; los átomos de emoji y los enlaces no reciben esa excepción.

## Almacén compatible y alcance de la migración

La ubicación normal de esta versión de NotDiscord es
`%LOCALAPPDATA%\NotDiscord\shared`. Su formato conserva los contratos de datos
necesarios para que una futura versión de Plus pueda adoptar ese mismo
almacén. El NotDiscord+ conservado sigue utilizando su anterior directorio
`data-plus`; no se modifica ese binario ni se le redirige por medio del acceso
directo.

`configuration.json` tiene `schema_version: 1`, un mapa `sections`, el registro
de directorios migrados y espacio para campos compatibles adicionales. Las
secciones mantienen los contratos existentes de `ui-settings.json`,
`media-settings.json`, `sound-settings.json`, `audio-settings.json` y
`navigation.json`. Incluye también los datos de selectores y las vistas de
notificaciones por cuenta. La apariencia, sonidos, dispositivos, volúmenes de
entrada/salida, volúmenes individuales y volumen global de transmisiones
continúan utilizando sus componentes actuales a través del adaptador de
almacenamiento.

Las escrituras se serializan dentro del proceso y sustituyen el archivo
atómicamente. Al guardar una sección se preservan los campos desconocidos de su
objeto, sin volver a incorporar entradas eliminadas de mapas conocidos. Un
formato ilegible o una versión de esquema incompatible no se sobrescribe.

La primera ejecución importa los directorios `data` y `data-plus` del antiguo
layout portable/proyecto y el directorio indicado por el lanzador anterior.
No se busca de forma recursiva en unidades ni perfiles de navegador. Antes de
importar conserva copias en `migration-backups`; el archivo más reciente decide
los conflictos por sección, con conservación de claves ausentes de volúmenes y
eventos. Los archivos anteriores permanecen. Una sección ya establecida en el
almacén compartido prevalece sobre fuentes antiguas. Los WAV referidos por los
ajustes se copian sin decodificar, volver a convertir ni reproducirlos.
Esta importación copia los datos anteriores al nuevo almacén de NotDiscord y
conserva las fuentes. No sincroniza las modificaciones que se hagan después en
el Plus antiguo ni escribe cambios de NotDiscord de vuelta en `data-plus`.

La sesión se conserva en `session.dpapi`, separada del JSON y protegida con el
mismo mecanismo DPAPI de Windows. La migración copia únicamente los bytes
cifrados. El marcador de migración impide recuperar una sesión antigua después
de cerrar sesión deliberadamente. Las conversaciones fijadas permanecen en
`pins.dpapi` con su cifrado de funciones, conservando el orden y las retiradas de
la copia más reciente y respaldando los conflictos. Los marcadores cifrados de
seguridad se combinan reteniendo el estado más restrictivo.

Los previews explícitos y sus fixtures conservan directorios aislados y no
inician la migración ni sustituyen una instancia de escritorio abierta. El
subproceso de login comparte el directorio elegido por su proceso padre y su
handoff seguro existente; no adquiere la exclusión de las ventanas principales.

## Exclusión y cierre ordenado

Un mutex de admisión serializa los inicios y otro mantiene la propiedad de la
instancia de escritorio por usuario. El protocolo y su espacio de nombres están
preparados para ambas variantes, pero en esta entrega sólo NotDiscord los
implementa. Entre instancias que implementan el protocolo, la nueva señala un evento de cierre y
espera la liberación de la propiedad antes de abrir preferencias, sesión,
conexiones o multimedia. No se termina forzosamente ningún proceso.

La instancia anterior intenta guardar los cambios de ajustes que estén abiertos
antes de cerrar. Si falla, muestra el error y permite reintentar, conservando la
propiedad mientras tanto. Tras cerrar la interfaz, el controlador recibe su
orden normal de cierre y libera voz, receptores, Gateway y recursos. La
propiedad se libera después de la espera de cierre y la finalización del
runtime. Los mutex de Windows no dejan un bloqueo persistente tras una caída.

Para ejecutables antiguos sin el nuevo evento existe una solicitud `WM_CLOSE`
limitada a ventanas y ejecutables NotDiscord de la misma instalación; se espera
la salida del proceso antes de migrar. Si tarda, se informa al usuario y se
permite esperar/reintentar. La nueva ventana explica que se cerró la anterior
porque se abrió otra versión.

Al abrir NotDiscord puede solicitarse el cierre normal de un NotDiscord+
antiguo que ya esté abierto en la misma instalación, antes de importar sus
datos. El sentido inverso no puede garantizarse sin actualizar Plus: el binario
conservado no conoce el evento ni el mutex nuevos. Por ello quedan pendientes
la exclusión bidireccional entre variantes y el uso continuo de un único
archivo/sesión por ambas. No se presenta una importación inicial como una
sincronización completa.

Las fuentes, manifiestos y bloqueo durante la instalación de actualizaciones se
documentan en [updates.md](updates.md). La correspondencia definida es
`NotDiscord` para la variante estándar y `codex/notdiscord-plus` para Plus.
La rama y el cliente de Plus no reciben esta entrega; la comprobación e
instalación nuevas se entregan únicamente con NotDiscord.
