# Corrección de carga inicial — NotDiscord 0.10.1

Fecha: 1 de noviembre de 2026, America/Lima. Actualización **Pequeña**, inferida por corrección acotada de la regresión de arranque de 0.10.0: **0.10.0 → 0.10.1**. Base `e3554a0106f8dce38082a7b2a41cfbd35187dba6`, rama estándar `NotDiscord`. NotDiscord+ y legacy quedan fuera del cambio.

## Evidencia y causa

Tras actualizar, el usuario observa servidores, conversaciones y amigos vacíos; el mensaje de error indica que cambió la conexión antes de enviar la operación. Posteriormente confirma que siguen llegando mensajes y puede abrirlos: el transporte de eventos funciona, pero falta el catálogo inicial.

El diagnóstico aportado contiene once eventos de 0.10.0, build `aa5b31239b57be9a607bcd5c4085d87d06f67755`, con un evento Controller/Recoverable/Dispatch unos tres segundos después del arranque. No contiene suficientes etapas para inferir por sí solo el estado del Gateway. No se copian conversaciones, capturas ni el archivo del usuario al repositorio, ni se utilizan cuentas.

La secuencia causal se confirma en las fuentes:

1. `__login`, `crates/app/src/controller.rs`, establece `ConnectionPhase::Connecting` y cierra temporalmente el gate HTTP.
2. El runtime emite `READY_CATALOG` **antes** de `Connection(Synchronized)` (`crates/core/src/gateway_runtime.rs`, rama READY).
3. El handler de `READY_CATALOG` (`crates/app/src/controller/incoming.rs`) conserva la semilla y llama inmediatamente a `sidebar()`.
4. Antes de esta corrección, `sidebar()` clonaba el cliente mientras el gate aún estaba cerrado. Ese clon conserva su ticket de generación; abrir el gate después no autoriza trabajo ya capturado.
5. Si cualquier componente de la semilla necesita completarse por HTTP, `load_sidebar` falla antes de devolver también los restantes componentes. La carga había consumido su plazo y su handler de fallo no la reprograma. El catálogo queda vacío aunque lleguen eventos de mensajes.

Esta regresión fue introducida en la protección final de generación de 0.10.0. La revisión anterior omitió el orden del arranque; compilar las pruebas anteriores no cubrió esa secuencia. El diagnóstico no indica pérdida de datos ni justifica cerrar sesión o borrar almacenamiento.

## Corrección

`Controller::sidebar()` conserva la demanda y programa `sidebar_wait` si el transporte está suspendido, **antes de clonar Api o marcar la operación en curso**. No captura ni lanza trabajo HTTP en esa fase. Después de `Synchronized`, el `tick` existente inicia una sola carga con un clon vigente del propietario de sesión.

Se conservan semilla, epoch, cancelación, deltas y límite de una carga en curso. No se cambia el orden del protocolo, no se abre HTTP durante Connecting y no se renuevan tickets antiguos. La protección contra colas anteriores, reenvíos ambiguos y ráfagas de reconexión permanece activa. No se añade un bucle de reintento a errores HTTP ni se reconstruye la sesión.

## Verificación y entrega

La revisión estática propia e independiente cubre READY_CATALOG parcial → Synchronized, incluidos ambos eventos sin yield, y la preservación de los tickets antiguos. Se añadió un caso de regresión en `controller/connection.rs` que comprueba demanda retenida, clon vigente frente a ticket anterior, carga simulada única y aplicación del catálogo. Ese caso se compiló, pero no se ejecutó; no inicia cuentas ni un servidor HTTP.

En esta corrección no se ejecutan pruebas, cliente, instalador, dispositivos ni cuentas de Discord. La confirmación funcional del usuario sigue siendo necesaria. No se modifica la instalación local ni sus datos desde el entorno de desarrollo.

## Publicación verificada

[NotDiscord 0.10.1](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.10.1), pública y sin draft/prerelease, con un único instalador. La etiqueta resuelve al commit compilado **`627ad5c59f9a9aea613942dd311b5764ea87f8f1`**, también build ID.

- `cargo check --workspace --all-targets --release --locked --offline -j1`: correcto en **47,91 s**, sin ejecutar destinos.
- `cargo build --workspace --release --locked --offline -j1`: correcto en **8 min 29 s**. Advertencias existentes, sin errores.
- **429 entradas de compilación** permanecieron estables. Inventario de fuentes SHA-256 `55347baf603d3a8af2286c7e7bedf4fa11255242f64e4344cc27a7dfdcf55e01`.
- Runtime preparado de 31 componentes x64 y payload de 41 archivos verificados. El EXE/addon del payload coincide con el build. Setup se compiló sin ejecutarlo.
- Asset remoto con tamaño/digest idénticos al local y etiqueta contrastada contra el commit. El manifiesto de actualización se publica después de esa verificación.

| Artefacto | Identidad |
|---|---|
| Setup | `NotDiscord-Setup-0.10.1.exe`, **118 873 891 bytes** |
| SHA-256 Setup | `3852df80b6ccf6b79f5a11710d713059c45a8034efcc76a497860147f9a49717` |
| SHA-256 EXE | `1bc1855dd3b169a687aa32a0ef0614581721d541b797e3abe1cd6b91720c08d9` |
| SHA-256 addon | `0ab369b57973d1837b68b34881f3168e1a68b2a94e7d3441c7a1607131cc75d5` |

El instalador conserva el manifiesto `release: null` de la compilación; no se reconstruye con su propio hash. Los registros y recibos quedan en `.tools/temp/current/release-0101/`, fuera de Git y del paquete. `evidence/` continúa sin seguimiento. Se contrastaron antes/después los 38 archivos de distribución de Plus y las referencias de Plus/legacy, sin cambios.
