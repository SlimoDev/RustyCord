# NotDiscord y NotDiscord+

Proyecto Windows en Rust, dentro de `NotDiscord-Rust/`. Las ramas mantenidas son `NotDiscord` (principal, versión normal) y `codex/notdiscord-plus` (Plus). La versión C# retirada se conserva únicamente como archivo de fuentes en la rama `legacy` de GitHub.

Las entregas de las dos variantes son:

- `NotDiscord`: instalador único `NotDiscord-Rust/dist/NotDiscord-Setup-0.4.0.exe`, con selección de carpeta y actualización automática posterior.
- `NotDiscord+`: `NotDiscord-Rust/dist/NotDiscord-Plus-20261025`, sin cambios en esta entrega.

NotDiscord utiliza configuración centralizada y sesión protegida por Windows en `%LOCALAPPDATA%/NotDiscord/shared`; migra las preferencias antiguas al abrirse, con respaldo. Plus mantiene sus datos anteriores: la sincronización continua y la exclusión bidireccional entre variantes requieren una futura actualización de Plus. El versionamiento y el actualizador de esta entrega corresponden únicamente a la rama `NotDiscord`; consulta `NotDiscord-Rust/docs/updates.md`.

Las herramientas de desarrollo locales están en `.tools`; no se publican junto con los datos, cachés o binarios. Consulta `NotDiscord-Rust/README.md` para compilar y empaquetar la rama elegida. El instalador reúne el runtime multimedia necesario y no depende de las versiones C# retiradas. El lanzador estándar abre la instalación registrada; si no existe, abre el instalador.
