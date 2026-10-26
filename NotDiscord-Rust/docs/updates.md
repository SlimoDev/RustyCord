# Versiones y actualización

Esta entrega actualiza únicamente **NotDiscord estándar**. La rama, el binario,
los accesos directos y los paquetes existentes de **NotDiscord+** se conservan sin
cambios. El Plus conservado todavía no incluye este actualizador ni participa en
la configuración, sesión o exclusión bidireccional nuevas; esa integración queda
pendiente de una actualización de Plus autorizada por el usuario.

Esta entrega actualiza sólo NotDiscord. NotDiscord+ mantiene su rama, paquete y
acceso directo anteriores; la correspondencia Plus de la tabla describe el
soporte preparado para una futura actualización autorizada. El Plus anterior
todavía no consulta este manifiesto ni participa en la exclusión compartida.

`version.json` es el manifiesto compilado y publicado. `schema` vale 1. Su
correspondencia es explícita y se valida tanto en el cliente como en el instalador:

| Variante | `variant` | Rama de GitHub |
|---|---|---|
| NotDiscord | `standard` | `NotDiscord` |
| NotDiscord+ (correspondencia futura, fuera de esta entrega) | `plus` | `codex/notdiscord-plus` |

La aplicación consulta el archivo de su propia rama mediante la API pública de
GitHub, sin credenciales de Discord ni autenticación de GitHub. La consulta corre
fuera del hilo de interfaz, a los cinco segundos de abrir una ejecución normal y
luego cada seis horas; los previews no consultan. El botón permite reintentar.
GitHub puede limitar las consultas anónimas y el error queda visible en su tooltip.
El botón «Actualizar» sólo se habilita para una versión nueva con paquete; el
control contiguo «↻» permite consultar o reintentar. La precedencia sigue SemVer
(mayor, menor, parche y prerelease numérico; los
metadatos `+build` no alteran el orden). No se muestran avisos repetidos: hay un
único estado de actualización en el pie de la aplicación.

Un manifiesto con una versión más reciente y `release: null` se muestra como
publicado pero con paquete pendiente; no habilita una instalación ficticia. Para
habilitarla, el publicador debe generar y publicar un ZIP y su SHA-256:

```powershell
.\prepare-update-release.ps1 -PackageRoot 'dist\paquete-compilado' -Tag 'notdiscord-v0.3.0' -OutputDirectory 'dist\release-notdiscord-v0.3.0'
```

El script sólo prepara archivos locales; no compila ni ejecuta el cliente, no usa
cuentas, no sube archivos y no publica releases. El ZIP resultante se publica como
asset de la etiqueta indicada en `SlimoDev/RustyCord`, y el `version.json`
resultante se publica en la rama que indica su propio manifiesto. La etiqueta no
es la fuente de selección de variante: la rama sigue siendo la fuente de verdad.
Hay que incrementar la versión antes de compilar y conservar esa versión en el
paquete. El manifiesto debe publicarse después de que su asset esté disponible.

La actualización descarga sólo componentes propios: ejecutable Rust, addon,
loader multimedia, icono, licencias, configuración de runtime, lanzador y versión.
Los motores de terceros no se redistribuyen. El índice incluye los hashes de los
motores existentes que la versión requiere; una instalación incompatible debe
recibir un paquete completo en lugar de sustituir esos motores automáticamente.

El cliente valida origen HTTPS de GitHub, variante, rama, tamaño y SHA-256 del
ZIP. Un instalador local incorporado al binario valida rutas, archivos, hashes y
compatibilidad antes de pedir el cierre. Rechaza rutas absolutas, traversal,
enlaces, nombres reservados, archivos ajenos al producto y paquetes excesivos.
La interfaz se cierra sólo después de recibir la confirmación del instalador.
Éste reserva la admisión compartida entre las dos variantes antes de confirmar
la entrega y adquiere la propiedad compartida tras la salida del proceso. Mantiene
ambas reservas durante la sustitución y cualquier restauración para impedir que
otra instancia abra los archivos a medias. Espera la salida ordenada del proceso, conserva copia de los archivos
reemplazados y trata de restaurarlos si falla una sustitución. Después vuelve a
abrir el mismo ejecutable con los mismos argumentos; los accesos directos siguen
apuntando a esa ruta. No sobrescribe datos, preferencias, sesión ni cachés. Un
fallo de instalación se registra en `.notdiscord-update/<id>/error.txt` y produce
un aviso de Windows; el staging y la copia de seguridad se conservan para revisar
un fallo. El hash protege la integridad respecto del manifiesto publicado, no
sustituye la confianza en quienes pueden publicar en el repositorio.

Si no se pueden guardar los cambios antes del cierre, el cliente cancela la
entrega mediante un marcador que el instalador observa durante su espera. La
aplicación permanece abierta y la reserva de admisión se libera sin esperar el
timeout de cierre ni modificar archivos instalados.

Por instrucción del usuario, el flujo se revisa y compila sin ejecutar pruebas
funcionales ni la aplicación. La instalación y el reinicio quedan pendientes de
su confirmación; un release sólo está disponible cuando se publican sus assets.
