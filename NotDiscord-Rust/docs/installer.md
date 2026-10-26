# Instalador de NotDiscord

Esta entrega distribuye únicamente `NotDiscord-Setup-<versión>.exe`. El directorio
utilizado para reunir el ejecutable y su runtime es un temporal de compilación,
no una distribución portable. NotDiscord+ y sus accesos directos se conservan.

La fuente del instalador está en `installer/NotDiscord.iss`. Se compila con Inno
Setup 6.7.3. La herramienta oficial puede instalarse de forma aislada bajo
`.tools/inno-setup`; antes de prepararla se debe comprobar la firma oficial del
descargable. No se ejecuta el instalador del cliente para compilarlo.

Flujo local, después de compilar Rust:

```powershell
.\package.ps1 -RuntimeRoot .\media-host\runtime -Destination '..\.tools\temp\current\package-031'
.\build-installer.ps1 -PackageRoot '..\.tools\temp\current\package-031' -OutputDirectory .\dist
```

El segundo script verifica la variante estándar, el manifiesto, los componentes
necesarios, las rutas sin enlaces y la lista de archivos permitidos. Incluye el
runtime multimedia local existente y los avisos/licencias presentes en ese
payload, junto con Unicode y Symphonia. Excluye datos, cachés, logs, volcados,
temporales y lanzadores CMD. La generación del instalador no acredita derechos
de redistribución adicionales de componentes de terceros.

Por defecto se instala para el usuario actual en su carpeta Programs de Windows,
con una subcarpeta `NotDiscord`, sin solicitar elevación. El selector permite
indicar otra carpeta base y añade `NotDiscord` si aún no es el último componente.
Las actualizaciones conservan la ruta elegida previamente. El modo opcional para
todos los usuarios utiliza Program Files de 64 bits y la elevación normal de
Windows. El paquete admite sistemas compatibles con ejecutables x64.

Se crean la entrada de desinstalación `NotDiscord.Standard` y accesos de NotDiscord
en el escritorio y el menú Inicio. Tras una instalación correcta se retira sólo
el antiguo acceso conocido `NotDiscord Rust.lnk`, comprobando que su destino
corresponde al cliente estándar. No se modifica `NotDiscord+.lnk`.

En modo por usuario, antes de sustituir esos accesos se obtiene únicamente el
argumento `--data-dir` de los accesos estándar conocidos. Se conserva como fuente
de migración en los nuevos accesos para que el propio cliente, al abrirlo el
usuario, traslade sus preferencias y sesión cifrada mediante el mecanismo ya
existente. El instalador no abre credenciales ni archivos de la cuenta. En modo
para todos los usuarios no se publica una ruta privada de un usuario en los
accesos comunes; los directorios anteriores permanecen conservados.

La instalación manual solicita el cierre ordenado mediante el evento y los mutex
existentes antes de sustituir archivos. No fuerza la terminación de procesos.
La desinstalación utiliza el mismo cierre y sólo retira los archivos registrados
de la instalación; no elimina preferencias, sesión ni otros datos del usuario.

El actualizador descarga este mismo EXE y utiliza su modo silencioso. La entrega
`ready/commit/cancel` permite guardar los cambios antes de cerrar el cliente.
El helper mantiene el mutex de admisión; el instalador verifica una autorización
local acotada, el proceso coordinador y los nombres de mutex, y adquiere la
propiedad durante la sustitución y cualquier rollback de Inno Setup. El modo de
actualización no abre automáticamente el cliente; el helper vuelve a abrirlo
después de verificar el resultado y liberar los bloqueos. Un fallo conserva los
datos y se comunica al usuario.

Se ha comprobado la compilación del código del instalador, sin ejecutarlo ni
realizar pruebas funcionales de instalación, actualización o desinstalación.
El comportamiento final de esos flujos queda pendiente de confirmación del
usuario.

Referencias oficiales de Inno Setup:

- [Descarga y editor de firma oficial](https://jrsoftware.org/isdl.php)
- [Instalación por usuario y elevación](https://jrsoftware.org/ishelp/topic_setup_privilegesrequired.htm)
- [Constantes Programs y Program Files](https://jrsoftware.org/ishelp/topic_consts.htm)
- [Subcarpeta al seleccionar destino](https://jrsoftware.org/ishelp/topic_setup_appenddefaultdirname.htm)
- [Ruta de una instalación anterior](https://jrsoftware.org/ishelp/topic_setup_defaultdirname.htm)
- [Arquitectura de 64 bits](https://jrsoftware.org/ishelp/topic_setup_architecturesinstallin64bitmode.htm)
- [Preparación, cierre y eventos de instalación](https://jrsoftware.org/ishelp/topic_scriptevents.htm)
