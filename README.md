# NotDiscord y NotDiscord+

Proyecto Windows en Rust, dentro de `NotDiscord-Rust/`. Las ramas mantenidas son `NotDiscord` (principal, versión normal) y `codex/notdiscord-plus` (Plus). La versión C# retirada se conserva únicamente como archivo de fuentes en la rama `legacy` de GitHub.

El formato de entrega de las dos variantes es:

- `NotDiscord` 0.10.0: instalador único `NotDiscord-Rust/dist/NotDiscord-Setup-0.10.0.exe`, con selección de carpeta y actualización automática posterior. El manifiesto `NotDiscord-Rust/version.json` indica la disponibilidad y los hashes de la publicación; `release: null` significa que el instalador todavía no está anunciado.
- `NotDiscord+`: `NotDiscord-Rust/dist/NotDiscord-Plus-20261025`, sin cambios en esta entrega.

NotDiscord utiliza configuración centralizada y sesión protegida por Windows en `%LOCALAPPDATA%/NotDiscord/shared`; migra las preferencias antiguas al abrirse, con respaldo. Plus mantiene sus datos anteriores: la sincronización continua y la exclusión bidireccional entre variantes requieren una futura actualización de Plus. El versionamiento y el actualizador de esta entrega corresponden únicamente a la rama `NotDiscord`; consulta `NotDiscord-Rust/docs/updates.md`.

Las herramientas de desarrollo locales están en `.tools`; no se publican junto con los datos, cachés o binarios. Consulta `NotDiscord-Rust/README.md` para compilar y empaquetar la rama elegida. El instalador reúne el runtime multimedia necesario y no depende de las versiones C# retiradas. El lanzador estándar abre la instalación registrada; si no existe, abre el instalador.

El [plan y estado de 0.10.0](NotDiscord-Rust/docs/plan-conexion-envios-adjuntos-ui-2026-10-31.md) documenta recuperación de conexión, envíos, adjuntos e interfaz, junto con las comprobaciones realizadas y la confirmación funcional pendiente. La revisión, compilación y pruebas sintéticas no acreditan por sí solas el comportamiento con Discord real.
