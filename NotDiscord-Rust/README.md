# NotDiscord Rust 0.13.0

Versión principal para Windows en Rust, rama `NotDiscord`. NotDiscord+ conserva su versión anterior en `codex/notdiscord-plus`; esta entrega no actualiza su rama ni su paquete.

0.13.0 es una actualización **Mediana publicada** de estabilidad y modularización: clasificación común de timeouts de voz, contexto de callbacks, IPC fuera del actor, eventos y recursos acotados y superficies de UI compartidas. Pasaron 1.021 pruebas del workspace, 61 del addon diagnóstico (solapadas con media), ambos probes N-API con fakes, build/check release y siete comprobaciones IPC. El [instalador Windows x64](https://github.com/SlimoDev/RustyCord/releases/download/NotDiscord-v0.13.0/NotDiscord-Setup-0.13.0.exe) se verificó en GitHub por tamaño y SHA-256, sin ejecutarlo. La [nota 0.13.0](docs/modularizacion-llamadas-0.13.0.md), el [informe T0](evidence/t0-review-0.13.0.json) y el [acta de publicación](evidence/publication-0.13.0.json) distinguen evidencia y límites. C5 y la causa de los cortes reales desde unos quince minutos siguen abiertos. El manifiesto anuncia el asset después de verificarlo; la compilación y Setup conservan `release: null`.

0.12.0 es una actualización **Mediana publicada**: ciclo de pantallas y sonido de fin, recuperación coordinada de Gateway/voz/visualización y suspensión de Windows, navegación del perfil, registros de grupo y círculo propio de voz. La [nota 0.12.0](docs/llamadas-presencia-navegacion-0.12.0.md) identifica implementación, pruebas y límites. Pasaron 995 pruebas del workspace y 55 del addon diagnóstico (solapadas con media), build/check release y siete comprobaciones IPC. El [instalador Windows x64](https://github.com/SlimoDev/RustyCord/releases/download/NotDiscord-v0.12.0/NotDiscord-Setup-0.12.0.exe) se verificó en GitHub por tamaño y SHA-256, sin ejecutarlo. C5, la pérdida física del micrófono al compartir, sigue abierto; se añade una observación local voluntaria sin grabar audio. El manifiesto de actualización anuncia el asset verificado; el compilado y el incluido en Setup conservan `release: null`.

0.11.0 es una actualización Mediana publicada: observación y salida de llamadas privadas, presencia visual inmediata, cabecera de vídeo ampliado y retorno del registro RAM. La [nota de implementación](docs/llamadas-presencia-navegacion-0.11.0.md) detalla las comprobaciones y los límites. El micrófono cuenta con diagnóstico y pruebas sintéticas; su incidente físico sigue pendiente de reproducción. El [instalador Windows x64](https://github.com/SlimoDev/RustyCord/releases/download/NotDiscord-v0.11.0/NotDiscord-Setup-0.11.0.exe) y sus hashes se verificaron en GitHub; la [nota de entrega](docs/entrega-local-0.11.0.md) registra la evidencia. No se ejecutó el instalador durante la entrega.

0.10.1 es una actualización Pequeña que corrige la carga inicial de servidores, conversaciones y amigos de 0.10.0. La [nota de corrección](docs/startup-catalog-0.10.1.md) documenta la causa, la espera hasta la sincronización y las verificaciones realizadas o pendientes.

0.10.0 incorpora recuperación conservadora de conexión, conciliación de envíos por nonce, favoritos GIF desde el chat, lector de adjuntos de texto y ajustes de reacciones, diagnóstico y colores. El [plan y estado de la tanda](docs/plan-conexion-envios-adjuntos-ui-2026-10-31.md) distingue los cambios, las comprobaciones y los casos pendientes de confirmación funcional.

## Compilación e instalador

Requiere Rust estable Windows x64 MSVC, herramientas C++ y SDK de Windows, WebView2 para el inicio de sesión e Inno Setup 6.7.3 para crear el instalador. Desde esta carpeta:

```powershell
cargo build --workspace --release --locked -j1
.\crates\media-addon\Prepare-Host.ps1 -ReferenceRuntime .\media-host\runtime -Destination .\target\prepared-runtime
.\package.ps1 -RuntimeRoot .\target\prepared-runtime
.\verify-package.ps1 -Root .\target\installer-payload
.\build-installer.ps1 -PackageRoot .\target\installer-payload
```

El runtime multimedia se prepara por separado mediante `crates/media-addon/Prepare-Host.ps1`. No se publica en Git ni se obtiene de las versiones C# retiradas. El empaquetador comprueba hashes, rechaza destinos existentes y no ejecuta el cliente.

## Datos y entrega

NotDiscord utiliza `%LOCALAPPDATA%/NotDiscord/shared/configuration.json` y una sesión protegida con DPAPI. La primera apertura migra los ajustes anteriores con respaldo. Los antiguos argumentos `--data-dir` identifican fuentes de migración; `--shared-data-dir` selecciona una ubicación compartida explícita. No se migran datos durante la compilación.

Plus continúa usando sus archivos anteriores. La sincronización continua y la exclusión bidireccional con Plus quedan pendientes hasta que se autorice actualizar esa variante. NotDiscord puede solicitar el cierre ordenado de una instancia anterior de su instalación y muestra el motivo.

En la publicación 0.12.0, `version.json` anuncia el instalador después de verificar el asset remoto por nombre, estado, tamaño y SHA-256. La etiqueta `NotDiscord-v0.12.0` fija las fuentes `9eb222ffd3a3e962106864317c6cb0a674f13209`; el manifiesto se publica en un commit posterior. El manifiesto compilado e incluido en Setup conserva `release: null` para evitar autorreferencia. El [acta de publicación](evidence/publication-0.12.0.json) distingue esos cortes y el acta local histórica. Las entregas publicadas usan un único Setup con el runtime requerido, tamaño y hashes verificados. El directorio de payload es sólo un temporal de compilación. El actualizador verifica el instalador, guarda los ajustes y utiliza su modo silencioso en la misma ruta elegida; no se publica una edición portable. No se publican cuentas, credenciales, datos, cachés ni herramientas locales.

- [Plan de modularización y estabilidad de llamadas del 9 de noviembre](docs/plan-modularizacion-llamadas-conservador-2026-11-09.md)
- [Reglas de desarrollo conservador](docs/desarrollo-conservador.md)
- [Corrección de carga inicial de catálogos en 0.10.1](docs/startup-catalog-0.10.1.md)
- [Plan y estado de conexión, envíos, adjuntos e interfaz de 0.10.0](docs/plan-conexion-envios-adjuntos-ui-2026-10-31.md)
- [Plan de correcciones y estado de 0.7.0](docs/plan-correcciones-interaccion-llamadas-gifs-2026-10-27.md)
- [Entrada y pegado](docs/input-0.7.0.md)
- [Geometría de animaciones](docs/animation-0.7.0.md)
- [Fechas y selector @time](docs/timestamps-0.7.0.md)
- [Menciones y catálogo de juegos](docs/game-catalog-0.7.0.md)
- [Diagnóstico local](docs/diagnostics-0.6.0.md)
- [Entrada nativa Alt y parches de dependencias](docs/alt-input-0.6.0.md)
- [Contrato de GIFs y favoritos](docs/gif-060-evidence.md)
- [Interacción, inventario de scroll y organización de datos](docs/interaction-and-shared-state.md)
- [Instalación y selección de ruta](docs/installer.md)
- [Versiones y actualizaciones](docs/updates.md)
- [Plan implementado y estado de 0.5.0](docs/plan-optimizacion-robustez-seguridad-2026-10-26.md)
- [Baseline y mediciones pendientes](docs/profile-baseline-0.5.0.md)
- [Diseño de firma pendiente de claves](docs/update-signing-design.md)
- [GIFs y limitación de Klipy](docs/gif-rendering.md)
- [Previsualizaciones de mensajes eliminados](docs/deleted-media-previews.md)
- [Adjuntos, suscripciones y límites](docs/uploads.md)
- [Corrección del bloqueo de arranque en 0.3.2](docs/startup-0.3.2.md)
- [Plan de skills y criterio de versionado](docs/plan-skills.md)
- [Mensajes y panel de usuario en 0.4.0](docs/ui-0.4.0.md)
- [Feedback de navegación y entrega de 0.4.1](docs/ui-0.4.1.md)
- [Evidencia de Markdown obtenida con REA](docs/rea-markdown-0.4.0.md)

Las comprobaciones realizadas y pendientes figuran en el plan de la tanda. En 0.11.0 pasaron las 939 pruebas de librerías del workspace, la compilación de producción, la comprobación de todos los destinos y el IPC local del paquete. Se ejecutaron probes con entradas sintéticas; no sustituyen confirmación visual ni de servicio con Discord real. No se realizaron pruebas con cuentas ni dispositivos físicos, ni se ejecutó el instalador.
