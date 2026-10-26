# NotDiscord Rust 0.3.0

Versión principal para Windows en Rust, rama `NotDiscord`. NotDiscord+ conserva su versión anterior en `codex/notdiscord-plus`; esta entrega no actualiza su rama ni su paquete.

## Compilación y paquete

Requiere Rust estable Windows x64 MSVC, herramientas C++ y SDK de Windows, y WebView2 para el inicio de sesión. Desde esta carpeta:

```powershell
cargo build --workspace --release --locked -j1
.\package.ps1 -RuntimeRoot .\media-host\runtime
.\verify-package.ps1 -Root .\dist\NotDiscord-0.3.0
```

El runtime multimedia se prepara por separado mediante `crates/media-addon/Prepare-Host.ps1`. No se publica en Git ni se obtiene de las versiones C# retiradas. El empaquetador comprueba hashes, rechaza destinos existentes y no ejecuta el cliente.

## Datos y entrega

NotDiscord utiliza `%LOCALAPPDATA%/NotDiscord/shared/configuration.json` y una sesión protegida con DPAPI. La primera apertura migra los ajustes anteriores con respaldo. Los antiguos argumentos `--data-dir` identifican fuentes de migración; `--shared-data-dir` selecciona una ubicación compartida explícita. No se migran datos durante la compilación.

Plus continúa usando sus archivos anteriores. La sincronización continua y la exclusión bidireccional con Plus quedan pendientes hasta que se autorice actualizar esa variante. NotDiscord puede solicitar el cierre ordenado de una instancia anterior de su instalación y muestra el motivo.

El manifiesto `version.json` identifica versión y rama. El actualizador descarga únicamente componentes propios y verifica tamaño, hashes y compatibilidad con el motor local. No se publican cuentas, credenciales, datos, cachés ni herramientas locales.

- [Interacción, inventario de scroll y organización de datos](docs/interaction-and-shared-state.md)
- [Versiones y actualizaciones](docs/updates.md)
- [Adjuntos, suscripciones y límites](docs/uploads.md)

La entrega se comprueba mediante revisión de código y compilación de producción y destinos auxiliares. No se ejecutan pruebas funcionales, dispositivos, llamadas ni cuentas. La confirmación del comportamiento corresponde al usuario.