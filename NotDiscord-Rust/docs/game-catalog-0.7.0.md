# Catálogo mínimo de juegos — 0.7.0

Implementación de la consulta C05. Contratos y evidencia estática de Discord en
`rea-time-game-2026-10-27.md`; no se usó una cuenta ni se ejecutó la API para
experimentar con respuestas/permisos.

El backend `controller/game_mentions.rs` sólo ejecuta los GET del helper core:
`games/autocomplete?q=…&filter_group=default` y `games?game_ids=…` con claves
repetidas. La segunda codificación tiene respaldo en el serializador público
Superagent y es una inferencia para el callsite del cliente, no una confirmación
de aceptación del servidor. Si el endpoint o formato no está disponible, el
resultado es un error finito con opción de reintento; no se inventan títulos ni
se consulta un perfil completo, juego instalado, página de tienda o una cuenta.

Cada acción conserva el stamp original: cuenta, generación, editor, apertura,
request UUID, revisión, consulta e IDs. El backend comprueba la sesión; la UI
debe cotejar el stamp completo antes de aplicar una respuesta. Una intención
nueva del mismo editor/apertura retira los observadores anteriores; cancelar
retira sus observadores y aborta trabajos sin consumidores. Logout cancela y
vacía todo. Las respuestas tardías tampoco cruzan la generación del controlador.

Presupuestos en memoria, sin persistencia de consultas:

- TTL de cinco minutos; máximo 64 búsquedas y 256 IDs, incluidas ausencias
  confirmadas por una lista válida, con LRU y cuota agregada de 1 MiB de tamaño
  serializado estimado. No equivale a RAM total; jobs y stamps tienen límites
  independientes y el transporte conserva sus límites existentes.
- Dos trabajos de red, ocho observadores por trabajo y ocho solicitudes en cola.
  Mismos endpoints comparten trabajo; IDs cacheados se quitan de la consulta de
  resolución. La UI agrupa los IDs visibles y aplica debounce a la composición.
- Veinte IDs/resultados por solicitud, consulta de hasta cien caracteres,
  stamp máximo 8 KiB, editor hasta 160 bytes y timeout de diez segundos.
- Sin reintentos automáticos. Un error, cola saturada, timeout o formato inválido
  responde sin mantener un spinner indefinido. La ruta usa `Api` existente,
  incluido el corte de seguridad, las colas y los rate limits compartidos.

La UI añade una caché de 256 IDs/1 MiB estimado y una única resolución visible
por lote de veinte IDs, además de la búsqueda de composición. La metadata del
mensaje tiene prioridad y evita la solicitud. Ausencia/error conserva texto
neutral y no se consulta otra vez por un repintado: cambiar de vista renueva la
intención. El conjunto de ausencias está acotado a 256 IDs. Los resultados se
cotejan contra el stamp completo antes de retirar la solicitud pendiente.
Un canal/cuenta/vista distinto invalida demandas anteriores. El compositor usa
250 ms de debounce; Enter/Tab seleccionan resultado y Escape cierra. Los nombres
con espacios se consultan completos y se insertan como `<@$id>` sin perfiles.
La selección conserva ese token; chips sin navegación usan cursor de texto.

Sólo se guardan ID, nombre acotado, icono validado y clasificación mínima
`restricted` que proyecta core. La elegibilidad NSFW parte de falso y se toma
exclusivamente de `users/@me.nsfw_allowed==true`; un `USER_UPDATE` propio la cambia
sólo si trae un booleano explícito. Se filtra al entregar cada resultado,
incluidos los cacheados, y también en UI. No se deduce permiso a partir del ID.

Errores de transporte se convierten a clases visibles genéricas; no se añaden
consultas, nombres, metadatos de cuenta, endpoints privados ni datos del mensaje
a los diagnósticos. No se ejecutaron pruebas funcionales o sintéticas; compilar
no confirma resultados de búsqueda, derechos de acceso ni interacción real.
