# NotDiscord C# — legacy

Archivo de las fuentes C# retiradas de NotDiscord. Esta rama no recibe desarrollo activo. La versión mantenida en Rust está en `NotDiscord` y NotDiscord+ en `codex/notdiscord-plus`.

## Contenido

- `terminal-native/src`: última interfaz propia WPF y lógica del cliente, con .NET 9 para Windows.
- `src/NotDiscord`: variante anterior WinForms/WebView2.
- Recursos, helper multimedia propio, código de emojis nativos y fuentes de pruebas/investigación asociadas.

No se incluyen sesiones, credenciales, perfiles de navegador, cachés, instaladores, resultados de mediciones ni binarios de terceros.

## Dependencias de reconstrucción

Se necesitan Windows x64, SDK .NET 9 y WebView2 Evergreen. Ambos clientes referencian `Microsoft.Web.WebView2` 1.0.4258.31 mediante NuGet; la variante WinForms conserva su `packages.lock.json`.

Para WPF, primero generar `terminal-native/native-emoji/bin/NotDiscord.Emoji.dll` con `terminal-native/native-emoji/build-native.cmd`, y después usar `terminal-native/build.ps1`. El primer script requiere MSVC C++ x64 y SDK de Windows; contiene una ruta de Visual Studio 2022 Community que puede necesitar adaptación.

La voz y la pantalla compartida requieren un host Electron compatible y el módulo nativo externo `discord_voice`. Esos motores no se publican aquí. `terminal-native/research/prepare-media-host.cjs` conserva el generador del bootstrap propio; necesita las rutas externas correspondientes. Node convencional no sustituye los callbacks nativos DAVE del host.

La variante WinForms también esperaba una instalación local de Vencord en `vendor/vencord/current`; la herramienta de restauración requiere su archivo ZIP local excluido.

Varias fuentes de pruebas usan DLL previamente compiladas mediante rutas `HintPath` de los directorios `build-*`. El archivo conserva sus fuentes, pero no afirma una reconstrucción automática de todas las pruebas. Este archivado se verificó mediante inventario, hashes y Git, sin ejecutar los clientes ni pruebas funcionales.
