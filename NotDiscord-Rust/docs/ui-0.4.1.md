# Feedback de navegación y entrega de 0.4.1

Entrega **Pequeña**, por elección del usuario como cambio menor: 0.4.0 → 0.4.1.
Sólo NotDiscord estándar, rama `NotDiscord`. NotDiscord+ conserva su versión.

## Causa y corrección

El helper introducido en 0.4.0 para enlaces de texto también se aplicó a filas
interactivas con fondo o resaltado: chats, miembros y vistas previas de respuestas.
Esas superficies ya comunicaban su interacción mediante el fondo, por lo que
el subrayado del nombre duplicaba el feedback.

Se distingue la superficie por su función, independientemente de si está
seleccionada en ese momento. Los controles con resaltado conservan su fondo,
cursor de mano, acciones y áreas interactivas, sin añadir subrayado de hover.
Los enlaces de texto y autores sin botón conservan el subrayado de hover/foco.
El subrayado escrito explícitamente mediante Markdown permanece como formato.

## Publicación e instalación

La skill `notdiscord-programar` incorpora la preferencia de entregar cambios
Pequeños mediante el instalador publicado y su manifiesto. El usuario actualiza
desde NotDiscord; la generación de la entrega conserva su instalación, accesos
directos y otras copias de uso. Generar Setup y ejecutarlo son pasos distintos.

GitHub almacena las fuentes y el instalador. El cliente instalado descarga un
Setup ya compilado; no construye el código Rust. Como el repositorio carece de
workflow de CI de compilación, esta entrega se genera en el entorno de desarrollo,
con un payload temporal. Si un futuro CI autorizado genera los artefactos de la
revisión exacta, la skill permite verificarlos y reutilizarlos.

## Comprobaciones

Pasaron la revisión de fuentes y cargo check --workspace --all-targets --release
--offline --locked -j1 (38,55 s), además de cargo build --workspace --release
--offline --locked -j1 (9 min 11 s). Los 151 archivos Rust conservaron sus hashes
durante la compilación y el empaquetado. El paquete de 39 archivos pasó
verify-package.ps1 y se compiló Setup mediante Inno Setup.

| Artefacto | Identidad verificada |
|---|---|
| NotDiscord-Setup-0.4.1.exe | 118 409 678 bytes; SHA-256 854c9b61132899a2a34db116ab319a5d965e9091aab8556108f515c9b746be5f |
| NotDiscord.exe | SHA-256 45f9035095d9ed4136c3c166ed4a0b1dc748ae69df7085ccdcf6043282e6febd |
| NotDiscord.Media.node | SHA-256 936db4bb5aefc990401296b86f167fa49c47ac326b5b5d886371dd9e76693a31, igual a 0.4.0 |

La [release 0.4.1](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.4.1)
contiene el instalador con estado uploaded, tamaño y digest SHA-256 remoto
coincidentes. El manifiesto se publica después de esa comparación. El cliente y
Setup se compilaron con release null para evitar el ciclo de su propio hash.

La skill pasó el validador oficial, la comparación de sus cuatro archivos entre
el repositorio y la instalación, y una revisión independiente de sus límites de
entrega. NotDiscord+ conserva su ref y los mismos 38 archivos del paquete previo.

No se ejecutaron el cliente, Setup, una cuenta de Discord ni pruebas funcionales.
La instalación y los accesos directos del equipo permanecen sin cambios. El
aspecto final y la actualización desde el cliente quedan pendientes de
confirmación del usuario.