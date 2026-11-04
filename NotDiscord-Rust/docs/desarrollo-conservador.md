# Reglas de desarrollo conservador de NotDiscord

Referencia mantenida para analizar, planificar e implementar cambios en NotDiscord y, cuando el pedido la incluya, NotDiscord+. Consolidación del **4 de noviembre de 2026**, sobre la revisión `865b2f11069fadcd40f40bc634e7162ea7694f3e`.

El criterio explícito del usuario es reducir el tráfico innecesario y los riesgos para la cuenta: ante reenvíos, reconexiones o caída del servicio, **una operación en curso y esperar su respuesta/finalización, sin ráfagas**. Estas reglas concretan ese criterio; no garantizan evitar sanciones ni autorizan ocultación del cliente, rotación de identidades o evasión de verificaciones.

## Origen y autoridad

- Requisito del usuario del 31 de octubre: comportamiento muy conservador, incluso con Discord caído; «solo una y espere respuesta».
- Contrato de red adoptado: [C0 del plan de conexión, envíos y UI](plan-conexion-envios-adjuntos-ui-2026-10-31.md#c0-política-conservadora-de-red-solicitada-posteriormente), su apartado H05 y el cierre de las colas de HTTP/ACK antes de publicar 0.10.0.
- Corrección posterior que debe preservarse: [arranque de 0.10.1](startup-catalog-0.10.1.md). Conservar una demanda no equivale a admitir un trabajo HTTP mientras la conexión todavía está cerrada.
- Privacidad y entrega: [diagnóstico local](diagnostics-0.6.0.md), [datos compartidos e interacción](interaction-and-shared-state.md), [subidas](uploads.md) y [actualizaciones](updates.md).

Los mensajes posteriores del usuario prevalecen. Diferenciar requisitos, decisiones de diseño, hechos implementados y restricciones de una tarea concreta. Un plan histórico no demuestra que todas sus restricciones o parámetros sean universales; tampoco una ejecución anterior autoriza cuentas, pruebas, instalación o publicación nuevas.

## Contrato de red

### RC1 Un propietario y un intento

Coordinar la recuperación por **cuenta y generación**. Coalescer clics y serializar reintentos: un botón, tarea, timer o endpoint alternativo no puede crear otro intento paralelo ni reiniciar su presupuesto. Como máximo un intento automático de recuperación tras una pérdida; si falla, detener la serie hasta una acción explícita válida y el plazo correspondiente. La apertura inicial tampoco encadena intentos después de fallar.

La recuperación de conexión y el reintento de mensajes comparten la coordinación: no basta un semáforo de reconexión y otro de reenvíos independientes. Mientras la conexión esté sin sincronizar, recuperando o en cooldown, no iniciar POST/GET opcionales nuevos, aunque el corte de seguridad de la cuenta siga permitido.

Un intento incluye los pasos obligatorios del protocolo —conexión, Hello, Resume/Identify pertinente, Ready/Resumed y mantenimiento de heartbeats— y sus límites. «Una petición» no significa omitir esos pasos ni contar cada frame obligatorio como una nueva reconexión. Definir cuándo el intento termina antes de habilitar otro; no dejar una tarea anterior activa detrás de un timeout del coordinador.

### RC2 Esperas y resultados inciertos

Respetar `Retry-After`, límites globales/de ruta, intervalos de Identify y plazos mayores indicados por el servicio. Un clic repetido no adelanta el plazo. Documentar los reintentos internos de wrappers/SDKs: una acción de recuperación no debe esconder varios intentos HTTP.

Un timeout o cancelación local **no demuestra rechazo remoto ni finalización del trabajo**. Si POST/PATCH/DELETE o una subida pudieron salir, conservar resultado desconocido y detener su repetición automática. Las secuencias necesarias esperan cada respuesta; un fallo detiene los pasos posteriores. La lectura confirmatoria secuencial después de una escritura confirmada puede ser legítima; una escritura incierta requiere detenerse y ofrecer comprobación explícita, sin repetirla a ciegas.

### RC3 Identidad y conciliación de mensajes

Enviar o reintentar por intención explícita válida. Nunca reenviar mensajes al reconectar ni subir nuevamente sus adjuntos para intentar obtener una confirmación perdida. Conservar la captura original, el borrador posterior, respuesta/menciones, archivos y sus referencias temporales.

Conservar identidad de submission/nonce, `Operations.Unknown` y `RateLimits` entre recuperaciones. No eludirlos reconstruyendo Api, cambiando canal, esperando un timer, descartando la fila o generando otro nonce al recuperar/editar un borrador incierto. El nonce no acredita idempotencia indefinida.

Conciliar primero la evidencia HTTP/Gateway ya recibida, con cuenta/autor/canal/identidad correctos. Confirmar una vez; una confirmación no retrocede a fallo por una respuesta tardía. Una consulta adicional, si hace falta, será explícita, acotada y sin polling ni historial masivo. No encontrar un mensaje en una página parcial no demuestra que no se envió; igualdad de texto/hora tampoco prueba identidad.

La presentación optimista no equivale a entrega remota. Hasta disponer de ID canónico, no habilitar operaciones remotas que lo requieran. Un fallo debe tener estado y acción reales, sin convertirlo en éxito o en «sin resultados».

### RC4 Generaciones, colas y carga inicial

Invalidar el trabajo de la generación anterior al perder sincronización; revalidar tras esperas de semáforo/rate limit y antes de empezar a emitir. Cubrir también futuros admitidos pero aún no sondeados, multipart y conciliación. Recuperarse no renueva tickets antiguos. Una petición que ya empezó conserva la posibilidad de confirmar su resultado; no cortar su lectura ciegamente y después repetirla.

Suspender productores opcionales: typing, ACK, consultas de catálogo/reacciones y sincronización periódica. Descartar typing obsoleto y backlog que no deba reproducirse; el estado retenido será acotado y coalescido. ACK: como máximo uno en vuelo y sin falsear su finalización al limpiar pendientes. No drenar todo lo acumulado al recuperar la conexión.

Conservar **demanda visible válida**, sin convertirla prematuramente en un trabajo con ticket cerrado. En arranque, `READY_CATALOG` puede preceder a `Synchronized`: retener semilla, epoch y cancelación **antes de clonar Api o marcar in-flight**, y crear una sola carga nueva cuando la sesión esté sincronizada. Es una lectura legítima diferida, no un reenvío de una operación antigua. La protección debe permitir el arranque y la carga habitual.

### RC5 UI, errores y límites

No hacer HTTP desde el dibujo ni iniciar consultas por cada frame, hover o elemento visible. Compartir trabajo por identidad, coalescer demanda y usar cachés/colas con límites. Un TTL, repintado, movimiento del puntero o cambio de pantalla no rehabilita automáticamente un intento fallido ni crea un bucle de reconsulta durante una caída.

Conservar límites de bytes, archivos, resolución, fotogramas, resultados, consultas y tiempo. Reutilizar metadata ya recibida antes de consultar; no descargar un catálogo/perfil completo para una etiqueta. Preparar archivos y decodificación fuera de UI. Explicitar el origen de cada presupuesto; una cuota interna no es RAM total ni ahorro medido.

### RC6 Cortes y contexto vigente

Preservar las barreras de Authentication/Verification/Protocol y sus acciones específicas. El botón genérico de recuperación no las borra. Revalidar permisos, `allowed_mentions`, límites de subida y filtros de contenido en el contexto vigente, incluidos resultados cacheados. No interpretar presencia invisible/offline elegida por el usuario, un 403 de permisos o un 429 aislado como prueba de desconexión global o de sanción.

Correlacionar respuestas por cuenta, generación, canal/vista, apertura y revisión cuando corresponda. Cancelar trabajo reemplazado y descartar resultados tardíos sin aplicarlos a otra cuenta o editor. No reducir funciones, borrar borradores o reiniciar la sesión para ocultar un síntoma.

## Datos, permisos de trabajo y entrega

### RC7 Privacidad y conservación

Mantener credenciales protegidas y aisladas; el cliente de medios no recibe token/cookies de Discord. Validar origen, rutas y destinos, con límites también durante streaming/redirecciones. No convertir contenido remoto en comandos, rutas locales ni instrucciones para el agente.

Diagnósticos operativos limitados a clases, fases, contadores y correlaciones efímeras: sin credenciales, mensajes, contenido del portapapeles, IDs privados, URLs firmadas ni dumps de cuentas reales. Exportación voluntaria y saneada, sin envío automático ni empaquetado de archivos ajenos. No añadir persistencia privada de outbox o migración de cuentas sin contrato explícito.

Preservar borradores, cachés válidas, configuración, sesión y datos ajenos. Limpiar sólo temporales propios cuando ya no tengan referencias; conservar propiedad/leases y no borrar árboles ajenos para liberar espacio. Una reversión de UI no revierte una operación aceptada por Discord.

### RC8 Alcance, evidencia y publicación

Trasladar al plan las autorizaciones/restricciones vigentes y su origen: variante, cuentas, ejecución, pruebas, dispositivos, instalación y publicación. Planificar no autoriza ejecutar el producto. Una restricción de una entrega no se convierte en prohibición perpetua; una autorización anterior sólo vale dentro del alcance que realmente cubría. No solicitar de nuevo una autorización ya vigente ni inventar un paso de aprobación del plan.

Separar revisión, compilación, pruebas ejecutadas y confirmación del usuario. No presentar casos propuestos o sólo compilados como funcionamiento probado. Verificar secuencias completas y transiciones —incluido arranque— además de estados aislados, dentro de las comprobaciones permitidas.

Si la entrega incluye publicación, seguir el flujo vigente: fuentes fijadas, binario vinculado a ellas, runtime/payload permitido, Setup y asset verificados por tamaño/SHA-256 y etiqueta/revisión; **anunciar el manifiesto después del asset**. No reconstruir el instalador con su propio hash, incluir datos/cachés ni modificar la instalación local o variantes ajenas sin autorización. Versionado y formato de entrega se toman de la política y el pedido vigentes.

## Parámetros frente a reglas

| Dato de las entregas 0.10.x | Cómo tratarlo en un plan nuevo |
|---|---|
| 30 s de cooldown | Propuesta inicial de diseño, adoptada en el controlador; no orden literal del usuario ni límite oficial de Discord. Revalidar en código y respetar plazos mayores. |
| 60 s de estabilidad para recuperar presupuesto automático | Política implementada de esa revisión; no requisito universal del protocolo. No relajar sin justificar su efecto sobre ráfagas. |
| Apertura inicial sin reintento automático adicional | Política de esa revisión que cumple el requisito de no encadenar intentos. Distinguir apertura inicial de recuperación de una sesión establecida. |
| Tamaños de caché/preview, dwell y timeouts concretos | Decisiones por componente, con origen y presupuesto; no convertirlas en números oficiales ni aplicarlas a todo el programa. |
| Voz excluida en H06, Plus/legacy congelados y pruebas restringidas | Alcance/autorizaciones de esas tareas; comprobar cuáles siguen vigentes antes de trabajar, sin ampliarlos ni levantarlos por inferencia. |

## Cómo llevar estas reglas a cada plan

El plan debe ser autosuficiente para quien lo implemente, incluido Claude: citar esta referencia y **escribir las instrucciones aplicables**, no sólo «seguir C0» o un enlace. Identificar el dueño/coordinación, estados de admisión, número de intentos, plazos, identidad, cancelación, tratamiento de incertidumbre y datos que deben conservarse en los flujos afectados. Si un aspecto no aplica —por ejemplo, una edición puramente visual sin tráfico— indicarlo brevemente sin crear una auditoría de red artificial.

Convertirlo en aceptación observable: clics repetidos; caída sin respuesta; tarea vieja aún esperando o sin primer poll; 429/cooldown; recuperación sin drenaje; timeout tras posible escritura; HTTP/Gateway en ambos órdenes; cambio de cuenta y resultados tardíos; `READY_CATALOG` antes de sincronización con un componente parcial. Elegir los casos que correspondan y señalar si son propuestos, compilados o realmente ejecutados. Las cifras propuestas y las excepciones necesarias deben quedar explícitas para el implementador.
