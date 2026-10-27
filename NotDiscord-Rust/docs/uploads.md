# Adjuntos y límites de subida

Política revisada el 26 de octubre de 2026. Se usa `premium_type` de la respuesta
de inicio de sesión existente y sus eventos `USER_UPDATE`; no se consulta la
facturación ni se hacen subidas de prueba. El servidor seleccionado aporta
`premium_tier` desde el catálogo que ya necesita el cliente. No se deduce una
suscripción a partir del avatar, banner, insignias o emojis recibidos.

El límite por archivo es el mayor entre cuenta y servidor: base 20 MiB,
Classic/Basic 50 MiB, Nitro hasta 1 GiB; servidor nivel 2 50 MiB y nivel 3
100 MiB. Una suscripción no confirmada utiliza la base y se identifica como tal.
Son valores de compatibilidad de la documentación actual; Discord puede aplicar
experimentos o condiciones más restrictivas. La respuesta remota es definitiva.
No se modifica un límite para intentar evadir un rechazo.

La interfaz comprueba cada archivo al adjuntar, incluyendo arrastrar/soltar y
portapapeles. Si una preparación asíncrona termina después de cambiar de canal,
la comprobación del destino se difiere al envío para evitar aplicar otro
servidor por error. El controlador vuelve a calcular el límite antes de enviar;
el transporte valida los tamaños de los archivos abiertos y mantiene esos
handles con flujos limitados a su longitud validada. No se carga el archivo
completo en RAM. Se conservan diez adjuntos por envío y un techo local de 1 GiB
por lote, señalado como límite local, independiente del derecho por archivo.

Un rechazo de tamaño (HTTP 413 o código Discord 40005) conserva el borrador y
explica cómo reducirlo. Las subidas no se reintentan automáticamente, tampoco
cuando una respuesta se pierde. Continúan vigentes la comprobación de permisos,
la cola limitada, la deduplicación y el respeto a los headers de rate limit.

Superar un tamaño no permite afirmar un baneo automático de la cuenta. Discord
documenta restricciones temporales de la API por exceso de solicitudes inválidas
401/403/429. Esta validación reduce envíos inválidos; no es una garantía contra
medidas de la plataforma ni contra otros riesgos del uso del cliente.

Fuentes oficiales:

- [Referencia de subidas](https://docs.discord.com/developers/reference#uploading-files)
- [Tipos de suscripción del usuario](https://docs.discord.com/developers/resources/user#user-object-premium-types)
- [Preguntas sobre archivos adjuntos](https://support.discord.com/hc/en-us/articles/25444343291031-File-Attachments-FAQ)
- [Nitro, Basic y Classic](https://support.discord.com/hc/en-us/articles/115000435108-What-are-Nitro-Nitro-Basic)
- [Mejoras del servidor](https://support.discord.com/hc/en-us/articles/360028038352-Server-Boosting)
- [Límites y solicitudes inválidas](https://docs.discord.com/developers/topics/rate-limits)

La documentación antigua de límites generales aún presenta otros números. Se
priorizaron la referencia API y las páginas específicas actualizadas. No se ha
verificado la aceptación de tamaños con ninguna cuenta ni mediante pruebas
funcionales.

## Corrección integrada en 0.5.0

El trabajo descrito en [límites pendientes](message-limits-pending.md) unifica
texto de envío y edición con la suscripción: 2000 como base y 4000 sólo para
Nitro completo. La política compartida valida el contenido final tras preparar
menciones/emojis/enlaces y centraliza el límite de diez archivos y 1 GiB por lote.
El editor conserva los adjuntos remotos ya alojados; editar texto no vuelve a
subirlos. No se usa el nivel de servidor para ampliar el texto.

La información de suscripción explícitamente desconocida vuelve al límite base;
un evento parcial que omite ese campo conserva el dato anterior. Los rechazos
conservan el texto y los adjuntos preparados. Una operación ya aceptada conserva
su política durante la espera de transporte y Discord valida el resultado final.

El usuario incorporó esta corrección al plan mediano 0.5.0 y autorizó compilar
y publicar su instalador al terminar; no se ejecutan pruebas funcionales ni se
utiliza una cuenta de Discord. La instalación se actualiza desde el cliente.