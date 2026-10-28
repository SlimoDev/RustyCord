# Versiones e instalación de NotDiscord

Esta entrega actualiza sólo **NotDiscord estándar**. NotDiscord+ conserva su rama,
ejecutable y accesos directos anteriores; todavía no participa en el actualizador
ni en la configuración y exclusión bidireccionales nuevas.

La distribución actual es exclusivamente el instalador
**NotDiscord-Setup-0.7.0.exe**. No se publica una edición portable de esta versión.
La instalación permite elegir una carpeta y, por defecto, utiliza Programs del
usuario para no requerir elevación. Una ruta protegida necesita el consentimiento
normal de Windows; el actualizador no intenta eludir UAC.

version.json utiliza schema 2, variant standard y branch NotDiscord. La aplicación
consulta ese archivo en la rama NotDiscord de SlimoDev/RustyCord mediante la API
pública de GitHub, sin cuenta ni credenciales. La correspondencia futura de Plus
sería variant plus / branch codex/notdiscord-plus; no se publica ni modifica Plus
en esta entrega. La versión instalada aparece en el título y se compara mediante
precedencia SemVer, incluidos prereleases numéricos.

La consulta se realiza fuera del hilo de interfaz, cinco segundos después de
abrir una ejecución normal y luego cada seis horas. Los previews no consultan.
El menú de usuario conserva «Buscar actualizaciones» para consultar manualmente.
La fila de Actualizar, su separador y el espacio que reserva se ocultan hasta
confirmar una versión más reciente con instalador válido. Ambas listas laterales
utilizan la misma condición para recuperar ese espacio. Una consulta inicial,
estar al día, un error sin actualización conocida o un paquete aún no publicado
no muestran esa fila.

Una vez confirmada la actualización, se conservan la fila, su progreso y el
control ↻ durante la descarga, preparación o reintento; desaparecen si una nueva
consulta confirma que ya no hay actualización. No hay avisos repetidos sobre la
misma versión. La entrega anterior de 0.3.2 a 0.4.0 reunió el ajuste del panel y las mejoras
de mensajes, Markdown y seguimiento del historial: se clasificó como Mediana por
elección explícita del usuario.
El criterio de versiones está en [el plan de skills](plan-skills.md).

En 0.4.1 se corrige el subrayado de controles con resaltado. Es una entrega
Pequeña (cambio menor) por elección del usuario. Su instalador se genera y
publica para actualizar después desde NotDiscord; la preparación de la entrega
conserva la instalación y los accesos directos del equipo.

La aplicación descarga Setup ya compilado y verificado. Subir código fuente
no cambia la aplicación instalada. Este repositorio no contiene actualmente un
workflow de CI que produzca el instalador: la compilación de desarrollo y el
payload temporal siguen siendo necesarios. Un futuro CI autorizado podría
producir esos mismos artefactos y evitar repetir esa compilación local.

El manifiesto publicado tiene esta estructura:

    {
      "schema": 2,
      "variant": "standard",
      "branch": "NotDiscord",
      "version": "0.7.0",
      "release": {
        "kind": "installer",
        "tag": "NotDiscord-v0.7.0",
        "url": "https://github.com/SlimoDev/RustyCord/releases/download/NotDiscord-v0.7.0/NotDiscord-Setup-0.7.0.exe",
        "sha256": "<SHA-256 del Setup.exe>",
        "bytes": 123,
        "executable_sha256": "<SHA-256 del NotDiscord.exe que instala>"
      }
    }

release null significa que el instalador todavía no está publicado. Los
manifiestos de esquema 1 se leen sólo para conocer su versión: este actualizador
no descarga sus ZIP. El cliente 0.3.0 antiguo no entiende el esquema 2 y necesita
la primera instalación manual del nuevo Setup.exe; las versiones instaladas con
este mecanismo podrán usar Actualizar en adelante.

Al pulsar Actualizar se descarga y valida el instalador HTTPS de la variante
correcta. Se comprueban origen de GitHub, tamaño, SHA-256 y formato de ejecutable.
La preparación se guarda en
%LOCALAPPDATA%\NotDiscord\updates\<identificador>, que es escribible aunque el
programa esté instalado en una carpeta protegida. No se ejecuta código remoto
antes de la acción explícita del usuario.

El helper incorporado al binario reserva Admission, confirma que está preparado
y espera el cierre ordenado del cliente. Si no pueden guardarse las preferencias,
el cliente cancela la entrega y permanece abierto; el helper libera las reservas.
Tras la salida, se entrega Owner al instalador mediante un lease validado, mientras
el helper conserva Admission. Inno Setup mantiene Owner durante los cambios y su
restauración, por lo que un fallo del helper no deja los archivos sin coordinación.

El instalador se ejecuta silenciosamente sobre la misma carpeta mediante /DIR.
Se conserva el ámbito de una instalación registrada (usuario o equipo); solicitar
elevación para escribir no convierte por sí solo una instalación de usuario en
instalación para todo el equipo. Setup no fuerza el cierre de otras aplicaciones.
Su modo de actualización no abre el cliente por su cuenta: el helper comprueba
el SHA-256 del ejecutable y el manifiesto instalados, libera la coordinación y
vuelve a abrir la misma ruta con los mismos argumentos. Configuración y sesión
continúan en su almacenamiento existente. Si falla la instalación, se muestra
el error y sólo se reabre la versión anterior automáticamente si sus componentes
de ejecución siguen idénticos. Los registros permanecen en la carpeta de preparación.

Para preparar la publicación, después de compilar el cliente y el instalador:

    .\prepare-update-release.ps1 -Installer 'dist\NotDiscord-Setup-0.7.0.exe' -Executable 'ruta-del-payload\NotDiscord.exe' -Destination 'ruta-nueva\version.json'

El script únicamente lee los ejecutables compilados y escribe el manifiesto; no
ejecuta, sube o publica nada, ni crea un ZIP. Se publica el Setup.exe como asset de
NotDiscord-v0.7.0 y después el manifiesto generado en la rama NotDiscord. Primero
se compila con release null, se empaqueta y se calculan los hashes; el manifiesto
con el hash del instalador no se vuelve a incluir en ese mismo instalador, evitando
un ciclo de hashes. Los hashes verifican integridad respecto de la publicación;
no sustituyen la confianza en quienes pueden publicar en el repositorio.

Por indicación del usuario no se ejecutan pruebas funcionales, el instalador ni
el cliente. La compilación y revisión estática no verifican instalación, UAC,
cierre, llamadas o reinicio reales. Su funcionamiento queda pendiente de confirmación.

## Endurecimiento de 0.5.0

El paquete consume un runtime preparado en staging nuevo y con inventario
permitido de archivos, tamaños y SHA-256. La verificación del paquete y la
compilación de Setup rechazan componentes inesperados o reparse points. La
preparación no decide la identidad del motor sólo por longitud de archivo.

Los nuevos staging del actualizador llevan marcador de propiedad y lease
abierto por cliente y helper. La retención limita a seis carpetas/2 GiB y retira
las que superan siete días cuando están inactivas; bajo presión retira primero
las propias más antiguas con al menos un minuto. Un staging activo o carpeta
ajena/legada no se borra, y si no hay espacio seguro la descarga se rechaza.
Sólo se retiran archivos de una lista plana conocida, sin borrado recursivo.

La política se aplica al código nuevo: las carpetas antiguas sin propiedad
reconstruible se conservan. La descarga de 0.5.0 desde un cliente anterior sigue
su helper anterior; no se cambia ese binario antes de instalar la entrega.

La [firma de manifiestos](update-signing-design.md) tiene un diseño de custodia,
rotación y recuperación pendiente de claves. Esta versión mantiene hashes y
HTTPS; no anuncia verificación de firmas ni protección completa ante compromiso
de publicación. No se creó una clave privada de producción en este trabajo.