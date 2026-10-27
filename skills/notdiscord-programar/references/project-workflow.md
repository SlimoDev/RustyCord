# Contratos del proyecto

Resolver el checkout mediante el contexto y Git. No depender de una letra de
unidad o ruta de otro equipo. La aplicación Rust se encuentra habitualmente en
`NotDiscord-Rust/`, con workspace `crates/core`, `crates/app` y `crates/media-addon`.
Comprobarlo en el checkout antes de usar comandos.

Las ramas conocidas son `NotDiscord` para estándar, `codex/notdiscord-plus` para
Plus y `legacy` para el archivo C#. Confirmar sus refs y el alcance actual; no
integrar una variante en otra por estar disponible. Si una edición está congelada,
conservar su rama, ejecutable, datos y accesos y contrastar los hashes pertinentes
al terminar. El archivo histórico no es una dependencia del runtime moderno.

Leer sólo la documentación pertinente de `NotDiscord-Rust/docs/`:

- `interaction-and-shared-state.md`: gestos, áreas de scroll, migración de
  configuración, DPAPI y coordinación de instancias. Una migración inicial no
  demuestra sincronización continua con un Plus antiguo.
- `gif-rendering.md` y `deleted-media-previews.md`: fuentes animadas frente a
  posters, límite de presentación de 30 FPS y límites de la caché de previews.
- `uploads.md`: límites y validaciones del destino. Verificar hechos cambiantes
  con documentación primaria; una hipótesis de autobaneo no es una causa probada.
- `installer.md`, `updates.md` y los scripts actuales: formato de entrega y
  manifiestos. `version.json` identifica la versión de la aplicación y su canal.

Para UI, recorrer `crates/app/src/ui/` y sus helpers compartidos antes de añadir
comportamientos por vista. egui no permite llamar de nuevo al mismo `Context`
dentro de una transacción `data_mut`, `input`, `memory` o equivalente: obtener
antes identificadores, viewport y demás metadatos. Una compilación correcta no
detecta ese bloqueo. Reutilizar áreas interactivas existentes y conservar las
prioridades de selección, arrastre, enlaces, menús y rueda.

En audio/vídeo, seguir configuración, señalización, receptor/decodificador y
representación por separado. No reiniciar voz al cambiar una visualización ni
tratar un cambio del indicador como solución de recepción. Un dato de actividad
local o medidor no acredita transmisión ni audio remoto.

Los diagnósticos deben limitarse a lo autorizado. Evitar tokens, memoria de
cuenta, chats y datos personales en registros, fixtures, commits y paquetes.
Las restricciones actuales viajan con la tarea; no asumir que una tarea nueva
autoriza usar cuentas o hacer pruebas prohibidas anteriormente.

Al recibir un plan, recuperar objetivo, revisión base, alcance, funciones que
conservar, evidencia confirmada/hipótesis, dependencias, aceptación, comprobaciones
permitidas, datos/migración, entregables y pendientes. Programación puede ajustar
detalles a la evidencia y debe registrar cambios materiales; no inventar una
aprobación pendiente del plan cuando ya se pidió implementarlo.
