# Parche de NotDiscord 0.6.0

Base: winit 0.30.13, revisión declarada e9809ef54b18499bb4f2cac945719ecc2a61061b.
Licencia Apache 2.0 íntegra en LICENSE. Modificados únicamente
src/event.rs, src/platform_impl/windows/keyboard.rs y src/platform_impl/windows/event_loop.rs,
con avisos de modificación en los tres archivos, el 27 de octubre de 2026.

La corrección transporta los commits nativos huérfanos Alt+numpad por la cola
ordenada de eventos de Windows, sin simular pulsaciones físicas ni hooks globales.
Evidencia, hashes de la base, contrato y limitaciones: [ledger del proyecto](../../docs/alt-input-0.6.0.md).