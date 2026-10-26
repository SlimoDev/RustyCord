# Vistas previas de mensajes eliminados

Esta corrección se aplica únicamente a NotDiscord; NotDiscord+ permanece sin
cambios. Se revisa mediante código y compilación, sin ejecutar la aplicación,
usar una cuenta ni realizar pruebas funcionales.

El controlador ya conservaba los metadatos observados del mensaje antes de
procesar su eliminación, incluyendo adjuntos y componentes multimedia. El
registro de cambios de la sesión clona ese contenido. La pérdida de la vista
previa ocurría en la presentación: la rama `m.deleted` sustituía los adjuntos por
`Adjunto conocido: …`, y el renderizador de contenido eliminado conservaba
únicamente texto para evitar iniciar nuevas descargas.

Ahora el renderizador de mensajes identifica explícitamente el canal y mensaje
que está mostrando. Cuando una tarjeta de un mensaje vivo dibuja realmente una
imagen o un fotograma, retiene una referencia a esa misma textura en una caché
de sesión. Al eliminarse el mensaje, el renderizador consulta exclusivamente
esa referencia conservada y pinta una imagen fija, manteniendo la relación de
aspecto. Los GIFs no necesitan seguir reproduciéndose.

La caché vincula cada vista previa al canal, mensaje y recurso multimedia
observados. Reutilizar una URL vista en otro mensaje no habilita la recuperación
de un mensaje que nunca se mostró vivo. El camino de mensajes eliminados no
llama a `texture`, `animated`, descarga, decodificador, reintento ni visor remoto.
Tampoco crea un archivo permanente de contenido eliminado.

El presupuesto independiente es de **64 vistas previas y 8 MiB de píxeles RGBA
estimados**, con retirada de las menos utilizadas. Sólo se conservan referencias
a texturas ya decodificadas; no se duplican los píxeles ni se leen de vuelta de
la GPU. Contabilizar referencias repetidas al mismo bitmap resulta conservador.
Este presupuesto no es una medida de la memoria total del proceso y se suma a
los presupuestos de recursos que ya existían.

Las referencias sobreviven a la navegación, minimización y retirada de la
textura de la caché general de imágenes, siempre dentro de ese presupuesto. Se
liberan cuando se sustituye la cuenta/sesión y al cerrar la aplicación. Si una
imagen nunca llegó a dibujarse, fue retirada por el presupuesto o pertenece a
un mensaje eliminado de una ejecución anterior, permanece su indicación de
adjunto conocido. No se afirma una restauración retroactiva ni persistencia de
estas vistas previas entre ejecuciones.
