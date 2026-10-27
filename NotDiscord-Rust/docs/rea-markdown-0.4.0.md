# Evidencia estática de Markdown: Discord web y NotDiscord 0.4.0

Investigación del 26 de octubre de 2026 con la skill `reverse-engineer-anything`
y REA. El objeto es el asset público de Discord recibido en esta tarea, sin
cuenta ni cookies. No se ejecutó su código, el cliente ni pruebas funcionales.
Las observaciones siguientes describen sintaxis y decisiones visibles en ese
artefacto; no acreditan ejecución, aspecto final ni equivalencia completa.

## Identidad y extracción acotada

- Asset: [web.ad0b95d8568e0f6b.js](https://discord.com/assets/web.ad0b95d8568e0f6b.js).
- Tamaño: **12 629 282 bytes**.
- SHA-256: `3f215e2a4d156120449c090a08973d2e7d96da0d9736a4bb79928829a0dfd4e0`.
- Copia local: `.tools/temp/current/ui-040/rea-public-discord/`, fuera del payload
  y de los archivos del producto.

El análisis completo inicial agotó el presupuesto de memoria antes de parsear
fuentes. El agente coordinador conserva su recibo
`ev_93844f88ea5f86c4f1b2fe1ace6db964ea84730068a8b9c9b022798c44687e18`.
Ese ID no estaba retenido en la conexión del agente que extrajo los módulos;
no se repitió el productor ni se presenta aquel intento como cobertura útil.

Para reducir el problema se tokenizó el mapa Webpack `I` con el Acorn 8.17.0
incluido en Node. La utilidad propia es
`.tools/temp/current/ui-040/extract-webpack-markdown.cjs`, SHA-256
`9c372529e72f7821d22bfeba0ffd3ddb63cfc04762980f4d6d489815566047cb`.
Recorrió 4 387 902 tokens e identificó 4865 métodos del mapa, conservando sus
rangos. Esto acredita los límites léxicos de ese mapa, no el análisis semántico
de todo Discord ni de sus chunks externos.

Cada fragmento conserva el método original byte por byte, envuelto únicamente
con el prefijo UTF-8 `28 7b` y el sufijo `7d 29 3b 0a`. Cada carpeta
`rea-markdown-extracts/module-<id>/` tiene `provenance.json` con rango original,
hash del método y hash del fragmento. Se comprobó el round-trip UTF-8 y se
parsearon los fragmentos como texto inerte. No se cargaron como módulos ni se
ejecutó su bootstrap. Ningún fragmento de Discord se copió al producto.

Los rangos siguientes son offsets de bytes, inicio incluido y final excluido:

| Módulo local a este asset | Rango original | SHA-256 del método original |
|---|---|---|
| 223637, cabeceras | 6006513–6006818 | `a5e6b57bce976a5d29d296c2004120c0b4025b46607d014582e35c8ed3a31e2a` |
| 694403, enlaces enmascarados | 6006819–6010659 | `6b4ecd253048c7e82e633a8c16bf91834b1450e96f4e0dc383a09954bb906603` |
| 873879, listas | 6010660–6012376 | `cc42e18b7ade1b2138d1641062c106a91fc385c1eac49389ce0cc1ba47ac5a76` |
| 247186, representación | 6012377–6019698 | `550215ed49111053d6d75ba8315f9479ede725c39ebd2e500dd95ac62bd91708` |
| 999915, conjuntos de reglas | 6019699–6034084 | `dbbb9a3f49ef4d295a856bbf9bc8d656a36a76e79b9f865a1c870bf707a3ce80` |
| 683412, subtexto | 6034374–6034764 | `79c7889c116a433379d6cbdd187ff623027ed1545d0d07710d3413e2efcaa5d5` |
| 46054, selección de parsers | 6035943–6040534 | `04eb760cc1d0182ef180615853395c2536cab8b1745b8074a3be61e161c7d01f` |

## Recibos REA y cobertura

El primer análisis acotado seleccionó seis fragmentos: 223637, 694403, 873879,
999915, 683412 y 46054. REA parseó **6 fuentes, 25 263 bytes y 8115 nodos AST,
sin fallos de parseo**. El análisis del renderer seleccionó después únicamente
247186: **1 fuente, 7327 bytes y 2555 nodos AST, sin fallos de parseo**.

| Evidencia | ID |
|---|---|
| Análisis de las seis fuentes | `ev_a27a899977ef658157351a110c430332495496e9cd31ca8e4ce43952cf39a83b` |
| Resumen del análisis | `ev_b4538ec417c66fafa28d25c86175b830032faff60754d9b37361d67bff5039ca` |
| Inventario paginado de fuentes | `ev_634830b1289e45d7ae9a48ee96e24e2ad55b84d4bc619b16cf8915fba522e333` |
| Fuente de cabeceras | `ev_647c411edd0f703b7f6f8d9bce9fc0500b158f11e35aa078aa09822b3f068f5b` |
| Fuente de listas | `ev_9a50ad964d0ded533c397fb28042da64d33cb0363f40ca8071afa6be86cf16ba` |
| Análisis del renderer | `ev_e6771446af69afd296f2dccb6d024d445fb348abb6b16d37e5317afac82402f2` |
| Resumen del renderer | `ev_fcaaa85ec4325fce19bd419b3770bc8b3ad6c7c9365c5b5b730bd4e51c5ee50e` |

REA trata los wrappers como fuentes JavaScript, con formato de módulo y exports
no resueltos. Los IDs numéricos y sus dependencias no quedaron enlazados como un
bundle ejecutable. Ambos grafos informan cobertura **parcial** y mantienen
incógnitas de resolución dinámica, flujo y módulos externos. Los cero fallos
de parseo no equivalen a cero incógnitas ni a cobertura completa.

Los siete recibos están exportados en
`.tools/temp/current/ui-040/rea-markdown-evidence-bundle.json`, 13 755 474 bytes,
SHA-256 `5dc6488f20d083f11ab469b81ac4db0881fe2a7b21ece8146b779cb80302cf62`.
Los resúmenes y las dos vistas de fuentes también se conservan por separado.
Estos archivos permiten reutilizar Evidence inline cuando otra conexión no
conserva los IDs; no hace falta repetir el análisis completo.

## Hallazgos causales y consecuencias para la implementación

Las conclusiones de esta tabla proceden de la lectura del método original
identificado y del parseo estático de su fragmento con REA. La columna de
consecuencia es una decisión de diseño para NotDiscord, no un resultado de
ejecución de Discord.

| Observación en el artefacto | Consecuencia para NotDiscord |
|---|---|
| 223637 condiciona las cabeceras a `allowHeading`, al inicio o después de salto; acepta uno a tres `#` seguidos de espacio. | Tratar las cabeceras como bloques con límites de línea; una coincidencia parcial en una frase no basta. |
| 873879 reconoce listas `*`, `-` o número y punto; exige `allowList`, rechaza `_listLevel >= 11`, conserva indentación y limita el inicio numerado entre 1 y 1 000 000 000. Desactiva cabeceras al parsear cada ítem. | Mantener estructura, indentación y numeración con límites explícitos; no aplicar indiscriminadamente las reglas de bloque dentro de un ítem. El guard se expresa sobre el contador de entrada, evitando confundirlo con el número de niveles ya renderizados. |
| 999915 contiene reglas de cita de una línea o líneas consecutivas con `>`, y cita del resto con `>>>`. Rechaza la cita si el estado ya indica `inQuote` o `nested`. | Las citas necesitan estado propio. `>> texto` no cumple la apertura observada y `> > texto` deja el segundo prefijo como contenido al bloquear el anidamiento. No inferir recursión ilimitada. |
| 999915 reconoce un bloque delimitado por tres acentos graves, lenguaje opcional y contenido separado; 247186 lo representa dentro de `pre` con un renderer de código. | Mantener código literal, separado de las reglas de formato y enlaces. La regex observada consume tres acentos; no demuestra que una secuencia de cuatro sea rechazada ni verifica por sí sola la precedencia de reglas heredadas. |
| 683412 reconoce `-# ` al inicio o tras salto, salvo `allowSubtext == false`; su contenido se parsea inline y desactiva subtexto interior. 247186 lo representa con `small`. | El subtexto es una regla de bloque contextual, no una sustitución global sobre cualquier guion y almohadilla. |
| 694403 requiere `allowLinks`, rechaza etiquetas vacías y enlaces enmascarados anidados, valida URL y protocolos, y permite formato inline de una lista explícita en la etiqueta. | Conservar validación y etiqueta estructurada; no abrir un destino sólo porque una regex encontró paréntesis. Respetar selección y texto visible sin alterar el destino validado. |
| 999915 define conjuntos separados para mensajes, biografías, títulos de embed, respuestas inline, topics y estados de voz. 46054 selecciona parsers para esos contextos. | Compartir parser y renderer con opciones por contexto; reutilizar componentes no significa activar las mismas reglas en todas las superficies. |
| 247186 representa nodos de lista con `ol`/`ul` y `li`, cabeceras con `h` más su nivel, cita con `blockquote`, tachado con `s`, subtexto con `small` y spoiler mediante un componente específico. | Evitar simular todo el formato con prefijos de texto; mantener nodos estructurales y estado de interacción para spoilers y enlaces. |

La validación de enlaces permite en ese artefacto los esquemas HTTP, HTTPS,
Discord, teléfono, SMS y correo. No se adopta automáticamente esa lista como
política de lanzamiento de NotDiscord: abrir esquemas y confirmar destinos
debe seguir la política de navegación ya existente del producto.

## Incógnitas y separación de verificación

La dependencia 478676, de la que se heredan reglas como énfasis, negrita,
subrayado e inline code, no está entre los 4865 métodos del mapa inspeccionado.
Puede estar en otro chunk: no se establece su ausencia del despliegue. Por ello
no se atribuyen a REA reglas detalladas de delimitadores heredados, todos los
casos de escapes ni la precedencia completa del parser original.

### Ampliación acotada para la regla de negrita

Ante una captura del usuario donde un cierre de negrita con espacio previo se
representa en negrita, se buscó la dependencia 478676 en exactamente los ocho
scripts siguientes a los tres primeros del HTML retenido. El HTML tiene SHA-256
`1b5526b59754b34b41c380309934b7cb89b9e307110d3ff235c088ae54ee5281`.
Se adquirieron anónimamente los índices 3 a 10, sin redirects, cookies ni
autorización, con límite de 10 MiB por respuesta. Todos devolvieron HTTP 200 y
`text/javascript`, sumando **42 165 bytes**:

| Asset bajo `https://discord.com/assets/` | Bytes | SHA-256 |
|---|---|---|
| `935812.136ada77a97748a6.js` | 1583 | `bf1267e79f42ecbcacdd2e532ae763c78abe67e85eabd135f04d5a2ec90eb192` |
| `434742.5cdde648b218a4fa.js` | 282 | `00ae70a1c5aa6d2819edd94cdf18e8c365834aea9de4714bd5fa8fa1750e3f8e` |
| `527542.281e49644b4d2d42.js` | 7570 | `0a331abe2099f6b68f947e5784a8035ddeb8301a09bfecdf33f002b174022c6e` |
| `948255.fde64b88d32defb4.js` | 1771 | `5e640a4431e555ceffc1fc7fb4ca34b963b9745847fbf0763ab4565c08569bc9` |
| `438027.d9881a5f46a8759c.js` | 25407 | `6e477e9d8191945bb6733498ddb91070c4564d13bb0434040c7d3258f9b5b3eb` |
| `240060.b8ecc1717610df21.js` | 3171 | `f61714b27f08a07aac14fcc41b8eb98489a537ca97fa84db865d259d5ef884c5` |
| `162412.aa9087f204b9b1a2.js` | 2273 | `43dd7d6f1b47746e728f4f664540b7091734ea690d0319d0c9c74e707c8764b7` |
| `99286.3b48322384eb245f.js` | 108 | `b08d756a82fb1329a68a9ea74b420f2e6cf024891db125af0ef72020f892338b` |

Ninguno contiene el literal 478676. No apareció un método de esa dependencia
para extraer y analizar con REA; este resultado sólo delimita la búsqueda en
esos ocho archivos. No se recorrieron los cientos de chunks restantes ni se
repitió el productor completo de 12 MiB. Las respuestas, URLs y hashes quedan
en `.tools/temp/current/ui-040/rea-markdown-base-candidates/acquisition.json`.
La eventual aceptación de espacios antes de cerrar negrita en NotDiscord se
fundamenta en la captura aportada por el usuario, separadamente de esta evidencia
estática: no se presenta como una regex original recuperada o probada.

Tampoco se midieron selección, clics, copiado, anchuras, fuentes, accesibilidad
o interacción real de los componentes de Discord. Las llamadas estáticas a
React no prueban qué combinación de flags está activa en una sesión concreta.
La comparación de nuestro código y su compilación corresponden a la tanda de
implementación; su funcionamiento visible sigue pendiente del usuario.
