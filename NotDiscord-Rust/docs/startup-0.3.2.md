# Corrección del arranque de NotDiscord 0.3.2

El usuario observó una ventana que aparece brevemente y un proceso que permanece
vivo sin abrir NotDiscord. La lectura de metadatos del proceso instalado 0.3.1
confirmó una ventana sin título que no responde. El SHA-256 de su ejecutable
coincide con el payload publicado. Esto acredita la copia de ese ejecutable,
sin verificar el funcionamiento de los demás componentes. No se leyeron
credenciales ni memoria del proceso, ni se inició la aplicación.

## Causa en el primer fotograma

`NotDiscordUi::update` llama a `autoscroll::begin_frame` antes de dibujar la
interfaz. La implementación de 0.3.1 calculaba dentro de `Context::data_mut` una
clave que consultaba `Context::viewport_id`. En las fuentes locales de egui
0.33.3, `data_mut` toma el bloqueo de escritura del contexto y `viewport_id`
intenta leer ese mismo contexto. El mismo hilo queda esperando un bloqueo que
no libera. La compilación no detecta este error de ejecución.

Las claves del controlador de desplazamiento reciben ahora un `ViewportId`
ya obtenido. Todas sus consultas al contexto y los metadatos de UI se calculan
antes de entrar en las transacciones `data`/`data_mut`. Se conserva la separación
por ventana, el propietario del desplazamiento, sus límites y cancelación.

## Lanzamiento desde el instalador

El instalador manual retenía los mutex Admission y Owner hasta
`DeinitializeSetup`, aunque podía abrir el cliente mediante `[Run]` antes de
ese evento. El cliente espera Admission antes de crear la ventana. Esto añade
una espera sin ventana mientras Setup siga abierto, independiente del bloqueo
de egui.

Ahora el instalador libera esos mutex al terminar correctamente `ssPostInstall`,
después de guardar archivos, accesos, INI, registro y datos de desinstalación,
y antes de la página final y del lanzamiento del cliente. `ssDone` sería tarde:
Inno ejecuta los programas postinstall antes de ese evento. La liberación final
es idempotente. El modo de actualización silenciosa conserva su protocolo de
coordinación, verificación y reinicio sin cambios.

El orden está contrastado con la [fuente oficial de Inno Setup 6.7.3](https://raw.githubusercontent.com/jrsoftware/issrc/is-6_7_3/Projects/Src/Setup.MainForm.pas)
y su [orden de instalación documentado](https://jrsoftware.org/ishelp/topic_installorder.htm).

## Alcance y recuperación

La corrección corresponde sólo a NotDiscord estándar. Conserva la ubicación
personalizada, las preferencias, la sesión protegida y el runtime existentes.
No modifica ni actualiza NotDiscord+.

El proceso antiguo detenido en el bloqueo no puede procesar su cierre normal.
Si continúa en el Administrador de tareas, el usuario debe finalizar ese
`NotDiscord.exe` antes de ejecutar el nuevo instalador. La entrega no termina
procesos a la fuerza ni abre una cuenta para verificar el arranque.

La causa y las correcciones se revisaron en el código; se compilan los destinos
de Rust y el instalador y se verifican los hashes de la entrega. No se realizan
pruebas funcionales del cliente, del instalador o de la actualización. La apertura
visible y el comportamiento real quedan pendientes de confirmación del usuario.
