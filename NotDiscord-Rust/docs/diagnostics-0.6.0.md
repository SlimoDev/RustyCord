# Diagnóstico local de NotDiscord 0.6.0

Los registros son operativos, locales y acotados. No contienen texto del chat o portapapeles, URLs, endpoints, nombres de usuario/juego, IDs de cuenta/chat, tokens, cookies, encabezados ni el payload arbitrario de panic. La API admite enums y números, no errores libres. El build opcional se admite sólo si es un hash hexadecimal de 40/64 caracteres.

Se instala un panic hook antes de iniciar tareas/UI. Tras localizar el almacén compartido se inicia el escritor en su subcarpeta `diagnostics`, con nombre aleatorio por ejecución. Los eventos distinguen arranque, error ordinario, fallo de tarea, transporte, salida del host, cierre forzado, panic y cierre limpio. Se preserva la clase y fase WebSocket antes del teardown; la salida del host registra la clase del control y su código de proceso disponible. Un marcador anterior abierto e inactivo sólo demuestra terminación no limpia: no permite afirmar que hubo un crash.

Presupuestos: ring de 256 eventos y cola de 256, entrada máxima de 4 KiB, archivo ordinario de 1 MiB; cuota de archivos propios de 16 MiB/14 días y hasta 128 ejecuciones. Hay una vía de emergencia pequeña para panic. El escritor ordinario es independiente de UI; `try_send` y `try_lock` no esperan a una cola/lock ocupados. Se deduplican repeticiones consecutivas durante un segundo. La residencia en RAM no equivale a esos presupuestos lógicos. Los fallos de disco dejan disponible el resumen en memoria, sin otra desconexión ni envío automático.

La limpieza sólo adopta nombres reconocidos con un marcador de propietario y formato verificado. Los archivos de ejecuciones activas están protegidos por un lease; se retiran payloads antes del marcador y nunca se usa borrado recursivo. Archivos ajenos, enlaces, formatos ambiguos o un directorio cuya enumeración exceda el límite no se borran como una forma de hacer espacio.

El menú de cuenta permite abrir «Diagnóstico local…», ver los eventos recientes, abrir la carpeta y exportar voluntariamente un JSON nuevo. Exportar serializa únicamente el ring tipado, no empaqueta archivos arbitrarios de la carpeta. No se exporta ni sube nada al publicar el instalador.

Los eventos de foco/minimización, cola de sonido, toast encolado/activado y transporte permiten ordenar un incidente. Sus correlaciones son efímeras; el número 0/1 de foco/visibilidad representa el estado booleano, no un usuario. Un sonido encolado no demuestra reproducción acústica. Los contadores no prueban causalidad entre un juego, una notificación y una caída de voz.

La llamada a `catch_unwind` supervisa futures Rust de órdenes, sin convertir una excepción nativa en recuperación garantizada. Los errores de `JoinHandle`, el timeout de cierre y un fallo de UI distinguen sus clases sin serializar excepciones privadas. No se generan minidumps de cuentas ni se ejecutan crashes de prueba en la instancia del usuario.

Verificación de esta entrega: revisión estática y compilación. No se ejecutaron el cliente, el instalador, pruebas de panic ni pruebas funcionales. La causa concreta del incidente de voz aportado necesita nuevos eventos saneados; el aviso anterior no conservaba suficiente información.