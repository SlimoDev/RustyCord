# Evidencia de timestamps y menciones de juegos de Discord

Investigación del 27 de octubre de 2026 mediante `reverse-engineer-anything` y REA. Se inspeccionó JavaScript público conservado como texto inerte. No se abrió Discord, ejecutó su código, usó una cuenta ni consultó APIs autenticadas. Las capturas del usuario aportan evidencia visual separada de esta inspección estática.

## Artefacto procedencia y límites

Se reutilizó el asset público [web.ad0b95d8568e0f6b.js](https://discord.com/assets/web.ad0b95d8568e0f6b.js), adquirido en la investigación anterior y conservado en `.tools/temp/current/ui-040/rea-public-discord/`:

- Tamaño: 12 629 282 bytes.
- SHA-256: `3f215e2a4d156120449c090a08973d2e7d96da0d9736a4bb79928829a0dfd4e0`, comprobado antes de extraer.
- Utilidad propia ya existente: `.tools/temp/current/ui-040/extract-webpack-markdown.cjs`. Tokeniza mediante Acorn, verifica round-trip UTF-8 y límites de métodos; no evalúa el bundle.
- Se identificaron los límites de los 4865 métodos del mapa Webpack tras 4 387 902 tokens. Eso no implica resolver sus dependencias ni analizar todos los chunks de Discord.
- Extracciones nuevas: `.tools/temp/current/plan-070-time-game/extracts/` y `support-extracts/`. Cada método conserva sus bytes y se envuelve únicamente con `({` y `});` más salto de línea. Cada fragmento tiene `provenance.json` con offsets, hashes de método/wrapper y transformación.

REA analizó una selección de diez fuentes (32 833 bytes; 10 726 nodos AST) y una segunda selección de cuatro dependencias de catálogo (6310 bytes; 1877 nodos). Ambas informaron cero fallos de parseo y **cobertura parcial**. Los IDs numéricos de Webpack, el flujo dinámico y los módulos no seleccionados no quedaron enlazados como una aplicación ejecutable. El segundo conjunto amplía una pregunta de catálogo; no repite el productor sobre el bundle de 12 MiB.

| Recibo REA | Identificador |
|---|---|
| Análisis principal | `ev_7b83b2595879fc05b97c6f470d1b3f03b9a2561e5e150f8e8217449f690ace35` |
| Resumen principal | `ev_a3581b6afa3d75a1b61b318df7f86f8f03c8291d1549ac2fd97534ba5e3d904f` |
| Vista del módulo de sintaxis de juego | `ev_f1bf5b5250d82fe123e92f4afbde3afa1fcf670a9b239d12bd542abdd645e2db` |
| Análisis de dependencias de catálogo | `ev_aaa54d12de8c269b7d62ed85c6bfce66de391772adbcf2ebeb5e58b0c3efbc0c` |
| Resumen de dependencias | `ev_472442612b1b827305dbdb0f650e3320758fdb487b8f0dfb8895021867d8bc1e` |

Exportación de seis registros, incluido el inventario paginado: `.tools/temp/current/plan-070-time-game/rea-evidence-bundle.json`, 15 900 613 bytes, SHA-256 `42e36c9bcf261d4ff22592856c4fffcbb362ca833c7448af1c8cbc9cfe481f66`. El contador de incógnitas registradas del bundle no sustituye las incógnitas semánticas de cada análisis: el primero informa 2571 y el segundo 429. Cero fallos de parseo no significa cobertura completa ni comportamiento probado.

## Localización reproducible de los métodos

Offsets de bytes del asset original, inicio incluido y final excluido. Los hashes completos del método y del wrapper están en cada `provenance.json` de las carpetas anteriores.

| Módulo | Rango de bytes | Función inspeccionada |
|---|---|---|
| 379418 | 6043263–6044421 | Gramática, formatos y conversión del timestamp |
| 785562 | 6042298–6043262 | Renderer y refresco de tiempo relativo |
| 827669 | 3318634–3318913 | Serialización y reconocimiento de `<@$id>` |
| 999915 | 6019699–6034084 | Reglas de parser para timestamp y gameMention |
| 247186 | 6012377–6019698 | Selección de componentes de presentación |
| 266645 | 6175199–6178661 | Etiqueta, icono, tooltip, copiado y apertura de perfil de juego |
| 442247 | 4953249–4953757 | Resolución del nombre/icono desde stores de juego/autocompletado |
| 311043 | 5021593–5024464 | Ingesta de `mention_games` desde mensajes y referencias |
| 243264 | 5024465–5025625 | Cachés de autocompletado y metadatos por ID |
| 966382 | 10055798–10056463 | Opciones `@game` y `@time` de la sugerencia |
| 471677 | 5029047–5031056 | Solicitud, normalización y temporización de búsqueda |
| 920495 | 5017004–5017639 | Consulta agrupada por IDs de juegos |
| 929396 | 5025626–5025991 | Normalización de consulta y supresión de prefijos |
| 997013 | 5018098–5021375 | Modelo de juego; distingue ID de juego de aplicaciones enlazadas |

Como anclas de integridad: método 827669 SHA-256 `21c5e9eae489c0cbfcf664635d759b0b9e8d0e5414d63acbf9f5aa048a82c5f5`; método 379418 `1db50f2b641d105684b977d8992ae71cf55c9251c4ea32da2865846438f8cc08`; método 471677 `188cc2ffe7b569a994c0fb27f12238ab24ed744ec700101a0e105fa19cdb981a`.

## Timestamps y diferencia con el texto de autocompletado

El módulo 379418 reconoce `<t:segundos:formato>` con formato opcional y timestamp decimal con signo, de uno a diecisiete dígitos. Convierte segundos a milisegundos, valida la fecha y usa `f` si se omitió formato. El parser 999915 conserva como texto una coincidencia cuya fecha no es válida. No reemplaza cualquier timestamp por la fecha actual.

Los formatos presentes en este artefacto son `t`, `T`, `d`, `D`, `f`, `F`, `s`, `S` y `R`. **`S` y `s` también existen aquí**: no limitar el soporte a una lista antigua de siete estilos. `S` usa fecha corta y hora con segundos; el tooltip utiliza la fecha completa `F`. `R` calcula tiempo relativo. El renderer 785562 refresca `R` con intervalos según distancia temporal y retira el intervalo al desmontarse; un timestamp absoluto no necesita ese refresco.

El texto interno de composición `<@time:...>` pertenece a una regla activada por `allowTimeMentionInput`; no es el token final del mensaje. `@time` es la entrada al editor de esa mención. Al finalizar se serializa `<t:...>`. La opción aparece en el módulo 966382 con tipo de autocompletado específico.

La captura del usuario muestra `<t:1793114201:S>` en NotDiscord y una fecha legible con segundos en Discord. Ese instante es `2026-10-27T15:16:41Z`; con UTC−05:00 corresponde a `2026-10-27 10:16:41`, consistente con la captura. El tooltip de la captura usa día de semana, mes y hora. El idioma/orden de fecha corresponde al visor; no debe fijarse inglés ni la zona America/Lima en el código de todos los usuarios.

Consecuencia propuesta: conservar un nodo tipado con timestamp/formato/rango original; formatear según zona/locale del visor, con tooltip completo y refresco acotado sólo para `R` visible. El AST cacheado no debe congelar el texto relativo ni depender de que el reloj cambie el contenido del mensaje.

## Menciones de juego

El módulo 827669 construye **`<@$ID>`** y extrae su ID decimal. No es una mención de usuario `<@ID>` ni un application command. El parser 999915 crea un nodo `gameMention` cuando el contexto permite juegos. El módulo 997013 conserva por separado el ID de juego y las aplicaciones enlazadas: no asumir que todos los IDs pertenecen al catálogo de aplicaciones detectables.

El renderer 266645 pide al resolvedor 442247 nombre e icono; el resolvedor prefiere el store de juegos y usa como alternativa el de autocompletado. El store de juegos 311043 ingiere **`mention_games`** desde mensajes, referencias y snapshots, además de resultados de consultas. Ese payload debe aprovecharse antes de emitir peticiones de resolución. La presencia de un usuario jugando es otra entidad y no sustituye esta metadata del mensaje.

Cuando dispone de datos, la etiqueta muestra nombre/icono y el tooltip contiene `@game` más el nombre, con demora configurada de 750 ms. La copia del objeto puede reconstruir su token canónico. Hay otra acción que abre una página/perfil de juego, pero es independiente de la representación inline.

El usuario pidió **excluir esa página/perfil**: NotDiscord necesita nombre, icono, tooltip y composición/autocompletado; no necesita replicar capturas, reseñas, tiendas, comunidad ni «Add to Profile». No debe presentarse un chip como botón de perfil si esa acción no existe. Un juego no resuelto requiere un fallback explícito y no debe desaparecer ni convertirse en ping a un usuario.

La captura confirma `<@$421150694683312138>` para Doki Doki Literature Club!, con etiqueta y tooltip. Se registra únicamente el ID público del juego; no se copian al repositorio nombres de personas, chats o la imagen de la conversación.

## Catálogo y composición de juegos

El módulo 966382 añade `@game` a las opciones de `@` bajo un ajuste del cliente original. Esa condición es un hecho de ese artefacto, no una condición de activación impuesta automáticamente a NotDiscord. Al elegirlo, la búsqueda tiene un estado distinto de la búsqueda de personas.

El módulo 471677 consulta `GAMES_AUTOCOMPLETE` mediante GET con `q` y `filter_group`; proyecta `id`, `name`, `icon` y `platform_availability`. El módulo de constantes 652215 del asset liga esa propiedad a **`/games/autocomplete`**, y `GAMES` a **`/games`**. Estas dos asignaciones se leyeron directamente en sus bytes originales; ese módulo grande no está entre las catorce fuentes analizadas con REA.

El módulo 929396 normaliza la consulta (trim, minúsculas, guion bajo como espacio) y la acota a 100 caracteres. 471677 controla búsqueda con ventanas de 200/500 ms y mantiene el resultado anterior mientras carga otro. 243264 mantiene cachés limitadas por consulta y por ID, con expiración y limpieza de logout. Son límites observados del cliente original, no benchmarks ni valores obligatorios para NotDiscord.

El módulo 920495 resuelve IDs mediante GET `/games` con `game_ids`, agrupándolos de veinte en veinte. Los helpers usan otros módulos de transporte, stores y filtros que no están completamente resueltos aquí. **No se consultaron esos endpoints**: su aceptación actual, permisos, reglas de `filter_group` y restricciones de contenido deben verificarse en la implementación autorizada, sin adivinar valores ni eludir el corte de seguridad. La evidencia estática acredita el camino del cliente, no una respuesta del servidor.

La propuesta es una caché mínima y acotada de juegos del servicio de Discord, primero metadata del mensaje y después resolución/búsqueda cuando sea necesaria. No escanear juegos instalados ni descargar una biblioteca/perfil completo para una mención. Serializar la selección como `<@$id>`, conservarla al editar y no incrementar contadores de menciones de personas.

## Separación de verificación

### Ampliación estática durante la programación del 28 de octubre

Se revalidó el mismo SHA-256 del asset y se extrajeron, con la utilidad inerte
anterior, los métodos 651188, 31498, 744700, 486020, 926675, 49491, 501592,
984703, 381438, 636537, 439864 y 978263. Cada fragmento conserva su procedencia
y rango en `.tools/temp/current/plan-070-time-game/contract-extracts/`.

REA analizó cinco fuentes seleccionadas (651188, 31498, 744700, 486020 y 926675):
17 945 bytes, 5055 nodos AST, cero fallos de parseo, cobertura parcial. Recibo
principal `ev_3f39f630c39d7fa1f437a2ce05bfd886b11367e9fdd651ab0bfa19d422865ec6`;
resumen `ev_584d453a2f25b909929d6e634ba4de1b16af598d2722b1f5db5cc304065904a1`.
Las otras fuentes se contrastaron directamente como bytes extraídos, sin atribuir
su cobertura a ese análisis REA. No se ejecutó el cliente ni el bundle.

651188 liga `DEFAULT` a `default`; 31498 usa ese grupo para `CHAT_MENTION`.
744700/486020 resuelven el icono como hash de `app-icons` o URL validada.
926675 aporta 77 IDs excluidos del autocompletado. 49491/501592/984703/381438
separan la clasificación adulta del permiso `nsfwAllowed`. La proyección mínima
conserva estas restricciones y sólo permite mostrar contenido restringido con
`nsfw_allowed` propio confirmado verdadero; no infiere permiso de un ID.

636537 entrega el objeto query al transporte importado 333405. Ese módulo no
está en el mapa inspeccionado: no se afirma haber resuelto todo su serializador.
La codificación de arrays con claves repetidas se contrasta con el
[serializador primario de Superagent](https://github.com/ladjs/superagent/blob/master/src/client.js).
Su aplicación a ese callsite es una inferencia de compatibilidad; la aceptación
actual de `/games` por el servidor sigue sin comprobarse. El cliente conserva
errores finitos y fallback, sin probar payloads con una cuenta ni reintentar
automáticamente un contrato rechazado.

El rango de fechas admitido por NotDiscord se acota al calendario 1–9999 usado
por su presentador Windows. El asset inspeccionado admite un rango JavaScript
mayor: esa diferencia es explícita; fuera del intervalo se conserva el literal.

Las capturas demuestran apariencia y los tokens que el usuario aportó. REA y los rangos originales demuestran estructura y decisiones visibles en este asset concreto. No se verificó ejecución de Discord, aceptación del servidor, comportamiento nativo del teclado, aspecto final de NotDiscord ni todas las combinaciones del parser.

El plan de continuación se mantiene en [el plan de correcciones](plan-correcciones-interaccion-llamadas-gifs-2026-10-27.md), apartado de replanificación posterior a 0.6.0. La implementación deberá conservar los presupuestos, permisos, cancelación y literalidad de código/escapes, y distinguir pruebas propuestas de comportamiento realmente confirmado.
