# NotDiscord y NotDiscord+

Proyecto Windows en Rust, dentro de `NotDiscord-Rust/`. Las ramas mantenidas son `NotDiscord` (principal, versión normal) y `codex/notdiscord-plus` (Plus). La versión C# retirada se conserva únicamente como archivo de fuentes en la rama `legacy` de GitHub.

Los accesos del escritorio y los lanzadores locales abren las dos variantes:

- `NotDiscord Rust`: `NotDiscord-Rust/dist/NotDiscord-0.3.0`.
- `NotDiscord+`: `NotDiscord-Rust/dist/NotDiscord-Plus-20261025`, sin cambios en esta entrega.

NotDiscord utiliza configuración centralizada y sesión protegida por Windows en `%LOCALAPPDATA%/NotDiscord/shared`; migra las preferencias antiguas al abrirse, con respaldo. Plus mantiene sus datos anteriores: la sincronización continua y la exclusión bidireccional entre variantes requieren una futura actualización de Plus. El versionamiento y el actualizador de esta entrega corresponden únicamente a la rama `NotDiscord`; consulta `NotDiscord-Rust/docs/updates.md`.

Las herramientas de desarrollo locales están en `.tools`; no se publican junto con los datos, cachés o binarios. Consulta `NotDiscord-Rust/README.md` para compilar y empaquetar la rama elegida. Los paquetes locales contienen su propio runtime multimedia y no dependen de las versiones C# retiradas.
