# Fechas y selector @time en 0.7.0

Alcance: C04 de la continuación posterior a 0.6.0, NotDiscord estándar. Se
reutiliza el reconocimiento tipado `core::message_atoms` y la evidencia estática
de Discord de [rea-time-game-2026-10-27.md](rea-time-game-2026-10-27.md). No se
ejecutó Discord, NotDiscord, el instalador, una cuenta ni pruebas funcionales o
sintéticas. Las comprobaciones de compilación se registran en la entrega principal.

## Presentación

Los nueve estilos `t`, `T`, `d`, `D`, `f`, `F`, `s`, `S` y `R` comparten un
presentador. El estilo omitido conserva el significado `f` del parser. Las
fechas se convierten a la zona local del visor mediante `chrono::Local` y usan
los formatos de fecha/hora de Windows; no se fija la zona America/Lima ni el
orden de fecha de otra región. `S` incluye segundos y fecha corta. El tooltip
muestra fecha completa, segundos y offset UTC. La notificación utiliza la misma
proyección, con tiempo relativo fotografiado al generar el aviso.

`R` usa el formateador relativo ICU del propio Windows cuando está disponible,
con el nombre regional obtenido del sistema. Sus firmas C, enumeraciones y
semántica se contrastaron con `icu.h` del SDK 10.0.26100.0 y las referencias
primarias de [ICU ureldatefmt](https://unicode-org.github.io/icu-docs/apidoc/released/icu4c/ureldatefmt_8h.html)
y de [ICU en Windows](https://learn.microsoft.com/en-us/windows/win32/intl/international-components-for-unicode--icu-).
La biblioteca combinada existe desde Windows 10 1903; se solicita opcionalmente
con `LoadLibraryExW` restringido a System32 y se comprueban los tres exports.
No se busca una DLL en el directorio del programa, no se redistribuye ICU y su
ausencia no impide iniciar NotDiscord. Cada formateador se libera tras usarlo;
la única referencia al módulo nativo y sus funciones dura hasta cerrar el proceso.

Si esa API no está disponible o rechaza el formato, se conserva una salida
relativa explícita: frases españolas/inglesas según idioma Windows y una
duración ISO 8601 con signo para otras regiones. El fallback de fecha absoluta
ante fallo de Win32 es ISO con hora de 24 horas. No se presenta ese caso degradado
como idéntico a la localización completa de Discord. Meses y años relativos son
aproximaciones de duración; no sustituyen el instante exacto del tooltip.

La caché de texto formateado contiene como máximo 256 entradas y se separa del
AST. Una nueva representación visible revalida tras un máximo de cinco segundos
o cambio de offset, de forma que la metadata formateada no queda congelada dentro
del mensaje. Sólo `R` visible solicita repintado, con intervalos de 1 segundo a
1 hora según distancia; no hay una tarea o temporizador por mensaje histórico.
El renderer registra el token fuente íntegro como átomo de selección del
historial. Copiar la fuente, editar o reenviar no reescribe el timestamp.

## Composición

Elegir `@time` abre un formulario local con fecha ISO, hora, selector de estilo y
preview. Su ámbito incluye apertura, cuenta, generación, canal, revisión del
borrador y rango de origen. Antes de insertar vuelve a comprobar ese ámbito y
el texto original exacto. Un cambio de vista/canal/cuenta, permisos o borrador
cancela el selector y conserva el contenido.

El formulario participa en el propietario común de teclado: recibe el foco
antes que el compositor, permite Tab y usa la acción primaria de Enter sin
convertir una confirmación IME en envío. Escape, Cancelar o cerrar conservan el
borrador. Confirmar inserta exclusivamente `<t:segundos:estilo>`, actualiza
menciones/rango de cursor y pasa por el presupuesto compartido de borradores.
No envía el mensaje, no guarda `<@time:...>` y no consulta ningún servicio.

Las fechas locales inexistentes por DST se rechazan con explicación; las horas
que ocurren dos veces requieren elegir primera o segunda ocurrencia. Fechas y
horas inválidas o fuera del rango admitido no producen un timestamp arbitrario.

## Pendiente de confirmación

La revisión estática cubre firmas nativas, límites de buffers, recursos,
selección, ámbitos e inserción. La compilación no demuestra apariencia,
localización final, eventos IME/Tab/Enter ni DST en Windows real. Quedan para el
usuario esas secuencias, cambio de zona/región, copia de estilos omitidos,
`R` pasado/futuro y carga de mensajes con AST ya cacheado.
