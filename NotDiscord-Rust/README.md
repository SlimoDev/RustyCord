# NotDiscord Rust

Versión principal mantenida del cliente Windows en Rust. NotDiscord+ se desarrolla por separado en `codex/notdiscord-plus`; las fuentes C# retiradas están archivadas en `legacy`.

## Compilar

Requiere Rust estable para Windows x64 MSVC, herramientas C++/SDK de Windows y WebView2 Evergreen para iniciar sesión. Desde esta carpeta:

```powershell
cargo build --workspace --release --locked -j1
```

También se puede usar `build.ps1 -Release`; `-Offline` es opcional si las dependencias están disponibles. La compilación no necesita ejecutar la aplicación.

## Paquete

Los motores multimedia de terceros no están incluidos en Git. El preparador `crates/media-addon/Prepare-Host.ps1` utiliza el runtime local existente de `media-host/runtime` o rutas explícitas `-ReferenceRuntime` y `-ReferenceNativeRoot` de un runtime compatible; no depende del antiguo proyecto C#.

```powershell
.\package.ps1 -RuntimeRoot .\media-host\runtime
.\verify-package.ps1 -Root .\dist\NotDiscord-Rust
```

El empaquetador comprueba las copias por SHA256, no sustituye destinos existentes ni recoge informes de versiones anteriores. El verificador sólo comprueba archivos y rutas: no inicia aplicaciones, dispositivos ni pruebas funcionales.

El lanzador del proyecto `ABRIR_NOTDISCORD_RUST.cmd` conserva la entrega actual `dist/NotDiscord-Rust-20261023-ui20` y los datos en `data/`. NotDiscord+ utiliza `data-plus/`. No se publican sesiones, credenciales, datos de usuario, cachés, herramientas locales ni motores binarios.
