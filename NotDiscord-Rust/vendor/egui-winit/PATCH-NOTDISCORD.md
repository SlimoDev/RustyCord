# Parche de entrada de NotDiscord 0.6.0

Base: egui-winit 0.33.3, revisión declarada por el paquete
`44cdd653e2317d300fb8a6c9c36b03f23991e803`, subdirectorio `crates/egui-winit`.
Se modificó sólo `src/lib.rs`, el 27 de octubre de 2026, para convertir la
extensión `winit::event::Ime::NativeText` en `egui::Event::Text`, manteniendo su
posición en la secuencia. No modifica foco, estado IME ni teclas físicas.

La licencia publicada es MIT OR Apache-2.0; esta distribución elige Apache 2.0.
Se conservan ambas licencias íntegras en LICENSE-APACHE y LICENSE-MIT,
recuperadas del commit oficial anterior. Se conservaron los demás archivos del
paquete; se excluyeron `.cargo-ok`, `.cargo_vcs_info.json` y el lock interno.

Hash original de `src/lib.rs` antes del parche:
`da39d0ca62ea970d1d780639b66af789ac37b16845728d6c32b01fc5e2e1cc3c`.
Evidencia y limitaciones: [ledger del proyecto](../../docs/alt-input-0.6.0.md).
