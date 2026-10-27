# GIFs y favoritos: evidencia y alcance de 0.6.0

Inspección estática del 27 de octubre de 2026. No se ejecutó JavaScript, Discord,
un navegador, llamadas ni dispositivos. No se leyeron sesiones, cookies de usuario,
cachés de perfiles o chats. Tampoco se enviaron peticiones a una API autenticada.

## Artefactos y contrato de favoritos

REA analizó el inventario del paquete de escritorio
`Discord/app-1.0.9265/resources/_app.asar`, SHA-256
`0025c02c7241045a12464309604ec4d5b8ec7ed2ce43cce05d5dacb8f253f672`.
Evidencia padre `ev_1828e033fca78c29f5b96052adb9147fd6e6ba0c4523008de29788666c29f8fa`;
vista de módulos `ev_67c37f98a0391a579908e890c27c1898e6bf5564aa95bf693702c9363785eed9`.
La selección inicial sólo analizó `package.json`: inventarió `bundle.js` y
`splashScreenPreload.js`, sin establecer el contrato de ajustes web. La ausencia
de cinco cadenas seleccionadas no se trató como demostración de ausencia global.

Después se leyó [el HTML público de Discord](https://discord.com/app), sin
cuenta ni cookies del usuario. Su SHA-256 es
`c5c057e81771bbc4687ff302f11bcc1f0b3ed533e8413d4e0299c1a9f2b3473d`.
Se adquirieron como texto tres scripts referenciados por ese HTML, con límite
por archivo y sin ejecutarlos. El contrato relevante está en
[web.ad0b95d8568e0f6b.js](https://discord.com/assets/web.ad0b95d8568e0f6b.js),
12629282 bytes, SHA-256
`3f215e2a4d156120449c090a08973d2e7d96da0d9736a4bb79928829a0dfd4e0`.
Los otros dos assets pesan 12506 y 8090 bytes; no se usaron para inferir el contrato.
Los archivos adquiridos permanecen en temporales ignorados, fuera del instalador.

Las posiciones siguientes son índices de **unidades UTF-16** de la cadena
obtenida decodificando ese archivo como UTF-8; no son offsets de bytes:

| Posición | Evidencia estática |
|---:|---|
| 606005 | La acción de retirar favorito elimina la clave del mapa y actualiza `favoriteGifs`. |
| 8558038, módulo 159201 | Copia binaria del subárbol y raíz parcial que contiene el campo modificado. |
| 8561052, módulo 761821 | La combinación sustituye los campos superiores presentes antes de aplicar el parcial. |
| 8545283, módulo 594061 | Escritura con revisión requerida, respuesta canónica y tratamiento de conflicto. |
| 8548522 | `out_of_date` significa descartar los cambios enviados, no repetirlos a ciegas. |
| 11299177, módulo 803805 | Esquema: campo raíz 2 de favoritos; mapa 1, tooltip 2; metadatos GIF formato/fuente/dimensiones/orden. |
| 11443491, módulo 813827 | Esquema de versiones: campo 3 representa la revisión de datos. |

Son hechos de un artefacto público concreto. No prueban que un servidor acepte
una escritura ni sustituyen una comprobación funcional. El contrato puede cambiar.

## Implementación

El transformador protobuf opera sobre bytes originales del subárbol 2: retira
sólo entradas con la clave exacta elegida, preserva las otras entradas, tooltip
y campos desconocidos, y no reserializa la vista filtrada del catálogo. Envía
ese único campo superior con la revisión de la lectura previa. Formatos ambiguos,
revisiones inválidas y tamaños excesivos se rechazan antes de una escritura.

La intención local se guarda antes de iniciar el cambio remoto. Sólo hay una
eliminación propia en vuelo. Después de un envío se consulta el estado canónico,
también si hubo un error incierto; no se repite PATCH automáticamente. Un conflicto
conserva el estado remoto confirmado. Si la eliminación se confirma y falla la
limpieza local, se informa por separado y se retiene la conciliación pendiente:
una lectura posterior vuelve a intentar la limpieza, sin restaurar el favorito
remoto ni repetir a ciegas su eliminación. Los ajustes tipo 2 recibidos por Gateway
invalidan la caché y actualizan las estrellas sin escribir.

El controlador es el único escritor del historial local. Recientes, importación
y estrellas usan operaciones con ámbito de cuenta/generación y confirman la UI
después de persistir. El formato mantiene compatibilidad con las listas anteriores
y añade rendition/dimensiones e intenciones de conciliación acotadas. Lectura y escritura comparten un tope de un MiB; si la lectura, el formato o los marcadores de conciliación fallan, el historial queda sólo de lectura y se muestra el error antes de cualquier cambio remoto. No se reemplaza un archivo ilegible con un historial vacío.

El descriptor común conserva página, poster, rendition y aliases acreditados por
metadata. No elimina query strings arbitrarios ni une páginas distintas sólo por
un poster compartido. Parser, mensajes, catálogo y favoritos lo reutilizan.
El reintento sigue usando la tarjeta existente; el resultado del catálogo se
correlaciona por cuenta/generación/apertura/petición/modo/consulta/límite.

La caché es LRU: ocho entradas, cuatro MiB estimados, sesenta segundos. Hay como
máximo cuatro trabajos y ocho observadores por trabajo; las solicitudes iguales
se reúnen y un prefijo mayor compatible puede reutilizarse. El grid conserva su
virtualización y sólo reconstruye la fusión de guardados al cambiar sus entradas.
El sobre de respuesta incluye hit/coalescencia, latencia del trabajo, elementos
y bytes estimados para futuras comparaciones; no es una medición de RAM residente.

## Verificación y pendientes

Se revisaron fuentes y diff y se añadió código de tests sintéticos para eliminación
precisa, conservación de bytes desconocidos, mapa vacío y revisión inválida.
No se ejecutaron esos tests, el cliente ni escrituras remotas. Compilación y
empaquetado los registra la entrega conjunta. El funcionamiento de favoritos,
GIFs y cualquier ahorro de latencia/CPU necesita confirmación del usuario.
