# NotDiscord+

Cliente de escritorio para Windows escrito en Rust. Esta rama `codex/notdiscord-plus` contiene la segunda versión de NotDiscord. El proyecto está en `NotDiscord-Rust/` dentro del repositorio.

## Compilación

Requiere Rust estable para Windows x64 MSVC, herramientas C++ y SDK de Windows. WebView2 se utiliza para el inicio de sesión. Las versiones de dependencias están fijadas en `Cargo.lock`.

Desde esta carpeta:

```powershell
cargo build --workspace --release --locked -j1
```

También puede utilizarse `build.ps1 -Release`. La compilación no ejecuta la aplicación, las pruebas ni los dispositivos. `--offline` es opcional si las dependencias ya están almacenadas localmente.

## Paquete y datos

El motor multimedia de terceros se prepara por separado; no se incluye en Git. `crates/media-addon/Prepare-Host.ps1` acepta rutas explícitas `-ReferenceRuntime` y `-ReferenceNativeRoot` para preparar un runtime local. Después de compilar:

```powershell
.\package-plus.ps1 -RuntimeRoot .\media-host\runtime
```

El paquete nuevo se escribe en `dist/NotDiscord-Plus-20261025` y el script rechaza destinos existentes. `ABRIR_NOTDISCORD_PLUS.cmd` utiliza `data-plus/` del proyecto; el lanzador dentro del paquete utiliza `data-plus/` del paquete. No se copia automáticamente ninguna sesión ni configuración de NotDiscord. La identidad de notificaciones y el acceso del menú Inicio también son independientes.

No se publican datos de usuario, credenciales, cachés, herramientas locales, paquetes compilados ni motores binarios de terceros. Los scripts históricos de empaquetado permanecen como referencia; para esta versión se utiliza `package-plus.ps1`.

## Cambios de esta rama

- Ventanas continuas de mensajes recientes, historial y saltos; actualizaciones puntuales y reconciliación durante la paginación.
- Renderizado de filas cercanas al viewport, geometría cacheada y selección conservadora que mantiene disponibles sus extremos y emojis.
- Proyecciones de canales, miembros y roles, demanda de rangos visibles y presencias procesadas por lote.
- Cachés acotadas de texto, avatares y recursos; prioridades de carga y cancelación del trabajo obsoleto.
- Banners de perfil globales y de servidor desde los datos existentes: póster estático, animación tras 150 ms de hover, GIF/WebP a un máximo de 30 FPS y pausa fuera de vista o al minimizar.

Los presupuestos de caché son límites de cada recurso, no una medición de la RAM total. Las filas aún no medidas utilizan alturas estimadas que se ajustan al hacerse visibles. Las animaciones conservan la duración de origen mediante un reloj común; los emojis mantienen su techo previo de 25 FPS.

Por indicación del usuario, esta entrega se valida mediante revisión de código y compilación. No se han ejecutado pruebas funcionales, llamadas, dispositivos ni una cuenta real para estos cambios; la confirmación del funcionamiento corresponde al usuario.
