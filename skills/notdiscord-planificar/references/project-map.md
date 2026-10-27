# Mapa de fuentes y contratos

Comprueba estas rutas en el checkout; el código y sus documentos mandan sobre
un mapa que haya envejecido. La aplicación suele estar en `NotDiscord-Rust/`.

En la tabla, `app/src/`, `core/src/` y `media-addon/` abrevian
`crates/app/src/`, `crates/core/src/` y `crates/media-addon/`, respectivamente.
Los nombres de módulos sin prefijo continúan en el directorio de su recorrido.

| Recorrido | Fuentes y documentos que conviene seguir |
|---|---|
| Estado y tareas | `crates/app/src/controller.rs`, módulos `controller/`, generaciones y cancelación; `core/src/models.rs`, `shared_list.rs` |
| Presentación y texto | `app/src/ui/mod.rs`, `chat.rs`, `chat/`, `mentions.rs`; parser compartido `core/src/markdown.rs` y `mentions.rs` |
| Recursos y memoria | `app/src/ui/resources.rs`, `resources/`, `image_spool.rs`, `avatar_cache.rs`, `message_cache.rs`, `ui/video.rs`; `docs/gif-rendering.md`, `deleted-media-previews.md` |
| Autenticación y almacenamiento | `authentication.rs`, `auth_job.rs`, `auth_lifecycle.rs`, `storage.rs`, `shared_state.rs`; `docs/interaction-and-shared-state.md` |
| Red y seguridad | `core/src/api.rs`, `api_operations.rs`, `gateway.rs`, `gateway_runtime.rs`, `rate_limits.rs`, `safety.rs`; `app/src/network_safety.rs`, `controller/request_policy.rs` |
| Voz, vídeo e IPC | `app/src/media.rs`, `controller/voice.rs`, `core/src/video.rs`; `media-addon/src/host.rs`, `session.rs`, `protocol.rs`, `native.rs`, `video.rs` y `host/main.cjs` |
| Actividad de juegos | `app/src/controller/game_activity*`, `game_rpc.rs`, `game_rpc/security.rs`, `protocol.rs` |
| Subidas | `core/src/uploads.rs`, API, compositor y controlador; `docs/uploads.md` |
| Actualización y distribución | `app/src/updates.rs`, `scripts/install-update.ps1`, scripts de paquete e instalador; `docs/installer.md`, `updates.md`, `version.json` |
| Dependencias y versión | `Cargo.toml`, `Cargo.lock`, manifiestos de cada crate/runtime; política de versiones y contrato en `docs/plan-skills.md` |

Las ramas conocidas son `NotDiscord`, `codex/notdiscord-plus` y el archivo
histórico `legacy`. Verifica sus refs y el alcance solicitado; la historia C#
no es una dependencia del runtime moderno. Las fuentes de ejemplos y probes
pueden aportar contratos sin ejecutarlos. Antes de cualquier diagnóstico
autorizado, comprueba sus efectos y las restricciones actuales.

En egui conserva anclas, prioridades de interacción y cancelación. No vuelvas a
entrar al mismo `Context` desde una transacción `data_mut`, `input` o `memory`.
En multimedia separa señalización, transporte, descodificación y presentación:
un indicador local no demuestra que el receptor remoto funcione.
