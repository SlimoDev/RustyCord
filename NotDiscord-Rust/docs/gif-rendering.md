# GIFs, tarjetas y fuentes de animación

Esta corrección corresponde a NotDiscord 0.3.1. NotDiscord+ no se modifica.

La tarjeta de un enlace se procesaba por separado de su medio: el mismo embed
producía el título/proveedor y un adjunto GIF. El renderizador ahora omite ese
envoltorio sólo cuando reconoce un GIF que ya tiene representación multimedia.
Los enlaces normales, artículos y campos estructurados de bots conservan su
contenido. Los componentes de archivo GIF usan el componente multimedia común.

La reproducción tenía otra causa independiente: la URL de la página, la
miniatura y el archivo animado podían confundirse. El modelo conserva ahora
`AnimationUrl` por separado de `PreviewUrl`. El catálogo y los embeds retienen
las variantes animadas declaradas; una miniatura no prueba que exista una
animación. Se separan las fuentes de póster y reproducción para evitar que un
póster estático sustituya a un GIF.

El resolvedor admite metadatos y datos estructurados de las páginas permitidas,
mantiene hasta 128 páginas, agrupa consultas simultáneas y respeta cancelación.
Valida cada redirección antes de seguirla. Informa de HTTP 403, caducidad,
límites del proveedor, formato no compatible y presupuesto local con mensajes
distintos. Conserva la vista previa y las acciones explícitas de reintento y
abrir el original; no añade solicitudes autenticadas ni claves de otra
aplicación. Un MP4/WebM no se trata como un archivo GIF ni se cambia su extensión
para intentar obtener una variante que no fue proporcionada.

Se admite WebP animado mediante el reproductor existente, con límites de tamaño,
fotogramas, píxeles procesados y memoria. Los GIF/WebP conservan sus duraciones
positivas declaradas; los fotogramas sin duración usan 20 ms. El reloj presenta
como máximo 30 fotogramas por segundo y salta los que queden atrás, sin alargar
la animación para imponer ese límite. No se modifican voz, cámara ni pantalla
compartida.

## Evidencia y límite de verificación

El 26 de octubre de 2026 se inspeccionó anónimamente la página pública que
proporcionó el usuario:
[stickergiant-buenos-noches](https://klipy.com/gifs/stickergiant-buenos-noches).
Respondió HTTP 403, `text/html`, 5546 bytes y sin redirección. No se ejecutó su
JavaScript ni se descargaron sus archivos multimedia. Por ello no se confirma
el formato exacto ni la reproducción de ese recurso concreto desde este
entorno, ni se atribuye de forma concluyente el fallo a Klipy.

La [documentación oficial de formatos de KLIPY](https://docs.klipy.com/migrate-from-tenor/response-objects/content-formats)
distingue las variantes GIF, vídeos MP4/WebM y previews de un solo fotograma.
Eso respalda conservar las URLs con su función original. Si el proveedor
únicamente entrega un vídeo o deniega acceso y no existe una variante GIF/WebP
declarada, permanece una limitación de reproducción informada; la corrección
del título o del indicador no se presenta como solución a ese bloqueo.

La verificación de la entrega comprende revisión estática y compilación, sin
pruebas funcionales ni uso de una cuenta de Discord. Las vistas previas de
eliminados y sus límites de sesión se describen en
[deleted-media-previews.md](deleted-media-previews.md).
