# Windows Alt+numpad: parche de NotDiscord 0.6.0

Fuentes emparejadas y fijadas por los parches locales del workspace:

- `winit 0.30.13`: revisión declarada `e9809ef54b18499bb4f2cac945719ecc2a61061b`;
  Apache 2.0 íntegra en `vendor/winit/LICENSE`.
- `egui-winit 0.33.3`: revisión declarada `44cdd653e2317d300fb8a6c9c36b03f23991e803`,
  subdirectorio `crates/egui-winit`. Licencia MIT OR Apache-2.0; se elige Apache
  2.0 para esta distribución. Se conservan ambas licencias íntegras, obtenidas
  del commit oficial mediante HTTPS público, sin usar cuenta.

No se copiaron `.cargo-ok`, `.cargo_vcs_info.json` ni locks internos. Las dos
copias se excluyen de los miembros del workspace. No se ejecutan ejemplos/tests
por usar estos backends como dependencias. Cargo.lock lo actualiza el integrador.

## Evidencia estática

El `keyboard.rs` original descarta WM_CHAR/WM_SYSCHAR cuando `event_info` es
None. El commit Windows de Alt+numpad puede llegar al soltar Alt sin key-down
pendiente. El puente egui-winit original entrega texto de KeyboardInput sólo
con state=Pressed. Añadir caracteres al final de un frame alteraría el orden de
Alt+64 seguido de Enter y no permitiría distinguir duplicados del backend.

La corrección conserva ese orden en el punto nativo. No simula pulsaciones
físicas ni composición IME: en egui 0.33.3 un ImeCommit aislado puede perderse
con cursor no inicial, y Enabled/Commit/Disabled activaría las barreras del
compositor que protegen Enter durante una composición real. El adaptador final
entrega texto normal mediante un contrato explícito entre las dos dependencias.

Hashes SHA-256 de los archivos originales antes de modificarlos:

| Archivo del proveedor | SHA-256 original |
|---|---|
| winit/src/event.rs | ff5c577343a725173a539c797e243fa8f6150c02181d904bddb9d82581a7a61e |
| winit/src/platform_impl/windows/keyboard.rs | 93188bdea60bce1014d624f289d25063b59a14985a318ca736bf62d8ba3e5d16 |
| winit/src/platform_impl/windows/event_loop.rs | f99c748e189ea8ce4bde08ec186ee27306756ce93d5235371e198c07e582c52d |
| egui-winit/src/lib.rs | da39d0ca62ea970d1d780639b66af789ac37b16845728d6c32b01fc5e2e1cc3c |

Sólo esos cuatro archivos del proveedor se modifican, con aviso fechado dentro
de cada uno. El resto del paquete se copió y contrastó por hashes. Las licencias
oficiales de egui tienen hashes `8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90`
(Apache) y `95ca92f5f8ea5231f1580b3a2a799e8260af3114b900e1def5355a7f44bcf60c` (MIT).

## Contrato de entrada

- Estado mínimo por KeyEventBuilder/HWND: Alt izquierdo y al menos una tecla
  numérica física detectada por el mapa de scancodes existente, independiente
  de NumLock. No se acumulan/convierten dígitos; sólo Windows decide el texto.
- Sólo WM_CHAR huérfano posterior a soltar Alt entra en el puente. Texto ya
  asociado a un key-down conserva la ruta normal; AltGr/Ctrl+Alt, WM_SYSCHAR,
  teclas muertas e IME real no se reinterpretan. Cambio de foco, cancelación,
  clic u otra tecla cancelan el intento.
- Se une un par UTF-16 válido. Unidades mal formadas o caracteres de control no
  se convierten en texto distinto. No hay buffers de dígitos ni cola nueva.
- AltText atraviesa la PendingEventQueue reentrante original. El guard nuevo
  no se conserva durante PeekMessageW, que puede invocar de nuevo el callback.
- La extensión local y oculta `Ime::NativeText(String)` representa texto nativo
  fuera de IME. El event loop la emite en su posición de la secuencia y el
  adaptador egui-winit la convierte directamente en `egui::Event::Text`.
  No cambia cursores, estados IME ni teclas. Enter posterior permanece separado.

La variante nueva es un contrato del fork emparejado, no una API añadida a
upstream. Otros consumidores exhaustivos de Ime necesitan admitirla si se usa
ese fork fuera de este workspace. Se revisaron los consumidores de Windows de
la distribución; la compilación final comprueba la integración real de targets.
No hay hooks globales, lectura de otras ventanas ni registro del texto tecleado.
El comportamiento no depende de transparencia ni de una sustitución especial de @.

## Comprobaciones y pendientes

Se revisaron el flujo de fuentes y la sintaxis de los cuatro archivos. No se
inició una ventana ni se hicieron pruebas funcionales. Los resultados finales
de compilación corresponden al integrador. Alt+64 y otros códigos, UTF-16,
cursor no inicial, modal, AltGr/IME y Alt+64→Enter siguen pendientes de confirmar
por el usuario. Inyectar Text("@") no demostraría entrada nativa de Windows.

Licencias oficiales: [Apache](https://github.com/emilk/egui/blob/44cdd653e2317d300fb8a6c9c36b03f23991e803/LICENSE-APACHE),
[MIT](https://github.com/emilk/egui/blob/44cdd653e2317d300fb8a6c9c36b03f23991e803/LICENSE-MIT).