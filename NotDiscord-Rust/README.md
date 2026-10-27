# NotDiscord Rust 0.4.1

Versión principal para Windows en Rust, rama `NotDiscord`. NotDiscord+ conserva su versión anterior en `codex/notdiscord-plus`; esta entrega no actualiza su rama ni su paquete.

## Compilación e instalador

Requiere Rust estable Windows x64 MSVC, herramientas C++ y SDK de Windows, WebView2 para el inicio de sesión e Inno Setup 6.7.3 para crear el instalador. Desde esta carpeta:

```powershell
cargo build --workspace --release --locked -j1
.\package.ps1 -RuntimeRoot .\media-host\runtime
.\verify-package.ps1 -Root .\target\installer-payload
.\build-installer.ps1 -PackageRoot .\target\installer-payload
```

El runtime multimedia se prepara por separado mediante `crates/media-addon/Prepare-Host.ps1`. No se publica en Git ni se obtiene de las versiones C# retiradas. El empaquetador comprueba hashes, rechaza destinos existentes y no ejecuta el cliente.

## Datos y entrega

NotDiscord utiliza `%LOCALAPPDATA%/NotDiscord/shared/configuration.json` y una sesión protegida con DPAPI. La primera apertura migra los ajustes anteriores con respaldo. Los antiguos argumentos `--data-dir` identifican fuentes de migración; `--shared-data-dir` selecciona una ubicación compartida explícita. No se migran datos durante la compilación.

Plus continúa usando sus archivos anteriores. La sincronización continua y la exclusión bidireccional con Plus quedan pendientes hasta que se autorice actualizar esa variante. NotDiscord puede solicitar el cierre ordenado de una instancia anterior de su instalación y muestra el motivo.

El manifiesto `version.json` identifica versión y rama. La distribución es un único `dist/NotDiscord-Setup-0.4.1.exe`, con el runtime requerido. El directorio de payload es sólo un temporal de compilación. El actualizador verifica el instalador, guarda los ajustes y utiliza su modo silencioso en la misma ruta elegida; no se publica una edición portable. No se publican cuentas, credenciales, datos, cachés ni herramientas locales.

- [Interacción, inventario de scroll y organización de datos](docs/interaction-and-shared-state.md)
- [Instalación y selección de ruta](docs/installer.md)
- [Versiones y actualizaciones](docs/updates.md)
- [GIFs y limitación de Klipy](docs/gif-rendering.md)
- [Previsualizaciones de mensajes eliminados](docs/deleted-media-previews.md)
- [Adjuntos, suscripciones y límites](docs/uploads.md)
- [Corrección del bloqueo de arranque en 0.3.2](docs/startup-0.3.2.md)
- [Plan de skills y criterio de versionado](docs/plan-skills.md)
- [Mensajes y panel de usuario en 0.4.0](docs/ui-0.4.0.md)
- [Feedback de navegación y entrega de 0.4.1](docs/ui-0.4.1.md)
- [Evidencia de Markdown obtenida con REA](docs/rea-markdown-0.4.0.md)

La entrega se comprueba mediante revisión de código y compilación de producción y destinos auxiliares. No se ejecutan pruebas funcionales, dispositivos, llamadas ni cuentas. La confirmación del comportamiento corresponde al usuario.
