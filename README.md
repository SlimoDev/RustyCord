# NotDiscord y NotDiscord+

Proyecto Windows en Rust, dentro de `NotDiscord-Rust/`. Las ramas mantenidas son `NotDiscord` (principal, versión normal) y `codex/notdiscord-plus` (Plus). La versión C# retirada se conserva únicamente como archivo de fuentes en la rama `legacy` de GitHub.

Los accesos del escritorio y los lanzadores locales abren las dos entregas actuales con datos separados:

- `NotDiscord Rust`: `NotDiscord-Rust/dist/NotDiscord-Rust-20261023-ui20`, datos en `NotDiscord-Rust/data`.
- `NotDiscord+`: `NotDiscord-Rust/dist/NotDiscord-Plus-20261025`, datos en `NotDiscord-Rust/data-plus`.

Las herramientas de desarrollo locales están en `.tools`; no se publican junto con los datos, cachés o binarios. Consulta `NotDiscord-Rust/README.md` para compilar y empaquetar la rama elegida. Los paquetes locales contienen su propio runtime multimedia y no dependen de las versiones C# retiradas.
