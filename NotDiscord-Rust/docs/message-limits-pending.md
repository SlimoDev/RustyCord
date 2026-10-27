# Límites de envío y edición pendientes de integración

Estado: corrección integrada en la entrega conjunta 0.5.0 del plan mediano.
El usuario autorizó ahora compilación y publicación del instalador al terminar,
sin ejecutarlo, hacer pruebas funcionales, usar su cuenta ni modificar Plus.

## Causa y alcance

El compositor y el envío utilizaban un máximo fijo de 4000 caracteres para
todas las cuentas. La edición mostraba ese mismo máximo y el controlador no
comprobaba de nuevo la longitud antes del PATCH. Las validaciones multimedia
ya tenían derechos por cuenta y servidor, pero debían mantenerse coherentes al
reintentar o cambiar de suscripción y destino.

La política compartida usa el premium_type recibido por la sesión y sus eventos
USER_UPDATE. Sólo Nitro completo (tipo 2) habilita 4000; sin Nitro, Classic,
Basic o suscripción sin confirmar utilizan 2000. El nivel del servidor amplía
el derecho de archivos cuando corresponde y no cambia el límite de texto.
Se cuentan unidades UTF-16 del contenido final, después de preparar menciones,
emojis y enlaces, conservando el criterio de conteo existente.

Los rechazos locales conservan el texto y los archivos preparados. No se dividen
mensajes ni se convierten automáticamente en adjuntos. Discord conserva la
validación definitiva y puede aplicar condiciones o experimentos específicos.
La corrección reduce envíos inválidos; no permite garantizar ausencia de sanciones.

La edición actual cambia texto y conserva los adjuntos ya alojados, sin volver
a subirlos. Por eso una bajada de suscripción no obliga a retirar un adjunto
antiguo al corregir el texto. No se introduce un editor nuevo de archivos.


El editor mantiene el documento mientras espera una respuesta correlacionada.
Una confirmación de éxito cierra sólo ese editor; el error permite corregir el
texto conservado. Token, cuenta, generación, revisión, canal y mensaje evitan
que una respuesta tardía cierre otra edición. Cerrar el editor pendiente no
cancela una escritura ya iniciada ni vuelve a abrirlo cuando llega su respuesta.
La interfaz cuenta el contenido normalizado, y el controlador realiza la
preparación definitiva una sola vez sobre el contenido original enviado.

## Estado de comprobaciones y entrega

La comprobación de esta corrección se limita a revisión estática de las fuentes
locales y del diff. Las comprobaciones de compilación se registran en el plan conjunto; no se ejecutan pruebas funcionales,
el cliente, Setup ni una cuenta de Discord. La entrega final corresponde al
instalador 0.5.0 y no cambia por sí sola la instalación.

La política de derechos se calcula con la información conocida al confirmar
la operación. Una operación que ya está esperando al transporte conserva esos
derechos hasta su respuesta: un cambio posterior queda sujeto a la validación
remota. No se añaden cancelaciones globales ni reintentos de escrituras.

El plan mediano ya recibido integra este trabajo; se resuelve conjuntamente la
versión y las comprobaciones según las instrucciones vigentes. La confirmación
funcional continúa a cargo del usuario.

## Fuentes oficiales consultadas

- [Límite de texto de mensajes](https://support.discord.com/hc/es/articles/360034632292-Enviar-mensajes).
- [Tipos premium](https://docs.discord.com/developers/resources/user#user-object-premium-types).
- [Subidas y adjuntos de mensajes](https://docs.discord.com/developers/reference#uploading-files).
- [Archivos y tamaños actuales](https://support.discord.com/hc/en-us/articles/25444343291031-File-Attachments-FAQ).