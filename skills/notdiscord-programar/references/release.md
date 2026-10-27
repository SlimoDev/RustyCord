# Compilación y entrega

Aplicar sólo las partes requeridas por la tarea. Leer los scripts y docs actuales
antes de ejecutar; las rutas y nombres de versión siguientes se resuelven desde
el repositorio, no desde esta skill instalada.

## Entrega y aplicación de cambios pequeños

En una entrega **Pequeña**, la preferencia del usuario es publicar el instalador
y el manifiesto para aplicar la actualización después desde el botón de
NotDiscord. Generar Setup no implica ejecutarlo ni reemplazar la instalación,
accesos directos o paquetes locales de uso del usuario. Mantener los temporales
de build y payload separados de esas instalaciones.

El cliente descarga un instalador ya compilado: subir fuentes a GitHub no genera
un binario por sí solo y la aplicación instalada no compila Rust. Antes de
entregar, comprobar si existe un flujo CI autorizado que produzca artefactos
para la revisión exacta. Si lo hay, se pueden recuperar y verificar esos
artefactos sin repetir la compilación local. Si no lo hay, compilar en el entorno
de desarrollo y publicar el instalador resultante. Crear una automatización CI
es un cambio separado; no se presupone al pedir una corrección pequeña.

## Procedimiento de publicación

1. Fijar tipo de actualización y versión según el usuario y la variante. Para
   agrupar cambios en una entrega usar su clasificación vigente; los cambios de
   requisitos posteriores prevalecen sobre una inferencia anterior. Antes de
   compilar, preparar el manifiesto del cliente con esa versión y `release: null`.
2. Integrar y revisar el diff. Excluir datos, cachés, herramientas, cuentas,
   logs y fixtures ajenos. Registrar qué fuentes corresponden al binario; una
   modificación que afecte a ese binario invalida el build anterior.
3. Ejecutar las comprobaciones permitidas. En el entorno actual se usa Rust
   MSVC x64; `cargo check --workspace --all-targets --release --locked` comprueba
   destinos sin ejecutarlos. `cargo build --workspace --release --locked`
   genera producción. Usar `--offline` cuando estén disponibles las dependencias;
   no iniciar el cliente o dispositivos como paso implícito de compilación.
4. Si corresponde un instalador, usar `package.ps1`, `verify-package.ps1` y
   `build-installer.ps1`, con un payload nuevo de compilación y el runtime
   autorizado. No publicar ese payload como portable si se pidió sólo instalador.
   Preservar selección de carpeta, ámbito, UAC y cierre coordinado. Compilar Setup
   no acredita instalación, actualización ni derechos de redistribución.
5. Con Setup ya generado a partir de esa compilación, calcular tamaño,
   SHA-256 del instalador y hash del ejecutable. Usar
   `prepare-update-release.ps1` para el manifiesto. No volver a incluir en el mismo
   instalador el hash de ese instalador, evitando un ciclo de autorreferencia.
6. Sólo con publicación autorizada, hacer commit/push de los archivos pertinentes
   a la rama correcta y crear la release con el asset correspondiente. Ante una
   respuesta desconocida, consultar el estado remoto antes de repetir la mutación.
   Evitar push forzado o cambios en ramas fuera del alcance.
   El título de cada release de NotDiscord debe ser exactamente `NotDiscord x.x.x`,
   sustituyendo `x.x.x` por la versión publicada, sin comentarios, descripciones,
   separadores ni otros sufijos en el título.
7. Comparar nombre, estado, tamaño y digest remoto con el asset local. Publicar
   después el manifiesto que anuncia ese instalador. Confirmar rama, versión y
   ausencia de cambios en la edición congelada. Una consulta GitHub autorizada
   no autoriza acceso a Discord ni pruebas del producto.
8. Entregar enlaces y resultados. Explicar los pasos manuales necesarios para
   una versión anterior bloqueada sin terminar procesos por la fuerza ni ejecutar
   un instalador como si fuera una prueba estática.

Mantener los archivos de configuración y sesión fuera de la carpeta reemplazada.
El actualizador guarda y cierra ordenadamente, verifica la entrega y reabre la
misma ubicación. No sustituir su coordinación por una espera fija, un kill
genérico o un reinicio de las conexiones mientras continúa usando archivos.
