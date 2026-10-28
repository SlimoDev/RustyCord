# Geometría de GIF y WebP en 0.7.0

Alcance: C03 del plan de continuación posterior a 0.6.0. Sólo NotDiscord
estándar. No se han ejecutado el cliente, dispositivos, cuenta, pruebas
funcionales ni fixtures sintéticas.

## Causa y contrato

El decodificador reducía el bitmap de cada fotograma para satisfacer el
presupuesto de memoria. La tarjeta y el visor usaban después ese bitmap como
tamaño natural, aplicando un límite de ampliación de 1. Por ello, una animación
con muchos fotogramas podía verse más pequeña que su póster aun conservando
el espacio exterior de la tarjeta.

El resultado del decodificador completo ahora contiene `canvas` validado y
`frames` por separado. GIF conserva el canvas lógico de su cabecera; WebP
conserva el canvas VP8X comprobado también contra el decodificador. Cada textura
sigue usando exclusivamente el bitmap reducido. Los emojis conservan su contrato
independiente de frames y no reciben nuevas descargas ni una caché duplicada.

`Pictures` mantiene como máximo 256 registros de geometría, con claves limitadas
a 4096 bytes y retirada LRU. Sólo almacena dimensiones y antigüedad. El cierre
de cuenta libera el mapa; detener, reintentar o minimizar una animación no borra
su geometría. No es almacenamiento permanente de archivos ni fotogramas.

La tarjeta fija su slot al aparecer, usando metadata válida y, en su ausencia,
el canvas ya conocido o el fallback existente. Conserva también el límite
natural de presentación: metadata declarada, canvas disponible o primer póster.
Los frames se contienen con su aspecto dentro de ese mismo límite. La resolución
reducida de los frames no cambia el tamaño visible; metadata de canvas con otro
aspecto corrige el contenido mediante contención, sin deformarlo ni mover las
filas siguientes. Los cambios explícitos de ancho de ventana siguen recalculando
el tamaño dentro del mismo contrato lógico.

Si no hay metadata ni un canvas todavía disponible, el primer póster es la
mejor dimensión acreditada de presentación. Una animación posterior no agranda
automáticamente ese límite. El visor permite ajustar o mostrar al 100 % el canvas
original una vez decodificado. No se inventa una dimensión original a partir de
una miniatura del proveedor.

El visor interpreta el 100 %, zoom, pan y límites con el canvas original de la
animación, incluyendo después de detenerla. Una corrección tardía del canvas
recalcula el factor relativo para conservar la extensión visible anterior y el
desplazamiento, y sólo entonces aplica los límites del nuevo aspecto. Hay un
único registro temporal del visor abierto; no se crean entradas por mensaje ni
temporizadores de geometría. Las imágenes estáticas conservan su política actual.

## Presupuestos conservados

- Canvas de animación de hasta 1024 píxeles por lado.
- Hasta 400 fotogramas y 64 millones de píxeles procesados.
- Hasta 8 MiB de píxeles reducidos por animación completa.
- Presentación como máximo a 30 FPS sin alargar las duraciones de origen.
- Cancelación por generación y rechazo de resultados antiguos antes de publicar.

Estos límites no describen la RAM total del proceso. Se mantienen las texturas,
la resolución reducida y el reloj existentes, sin añadir otra copia de los
frames. Sólo se adapta un literal de la fixture antigua al nuevo resultado
tipado para que los destinos de compilación sigan siendo compatibles.

## Comprobación y pendientes

La revisión estática sigue el canvas desde las dos cabeceras hasta la publicación
de texturas, tarjeta y visor, y conserva los rechazos de formato, secuencia,
presupuesto y cancelación. La compilación integrada corresponde a la entrega
principal; no se presenta como verificación visual.

Quedan para comprobación permitida por el usuario: GIF/WebP de igual canvas y
distinto número de frames, retrato/paisaje, Play/Stop/Retry, metadata ausente o
inconsistente, carga tardía, resize/DPI y visor con pan/zoom/100 %. No se afirma
que se hayan observado esas secuencias en ejecución.
