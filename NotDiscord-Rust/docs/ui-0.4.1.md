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

Se revisan las llamadas a los helpers y se compilan los destinos Rust y el
instalador, sin ejecutar el cliente o Setup. Se valida la skill y se comparan
sus copias instalada y del repositorio. No se usa una cuenta de Discord ni se
realizan pruebas funcionales: el aspecto final y la actualización desde el
cliente quedan pendientes de confirmación del usuario.
