# Diseño de firma de actualizaciones pendiente de custodia de claves

Estado en 0.5.0: diseño, sin clave de producción creada ni verificación de firmas
habilitada. Siguen activas las validaciones de HTTPS, origen, rama, versión,
tamaño, SHA-256 y ejecutable. Los hashes y el asset proceden del mismo origen:
no defienden por sí solos de quien pueda reemplazar ambos en la publicación.

## Contrato propuesto

- Ed25519 con clave pública fijada en el cliente y un key_id. La clave privada
  permanece en custodia offline del responsable de publicación o en un firmador
  separado con autorización explícita; nunca en Git, el instalador o los logs.
- Sobre firmado versionado con un payload exacto en base64 y firma sobre esos
  bytes. Evitar depender del orden de claves o espacios de JSON reconstruido.
- El payload incluye esquema, variante, rama, versión SemVer, tag/URL, bytes,
  SHA-256 del instalador, SHA-256 del ejecutable, hash del inventario del runtime,
  versión mínima del verificador y número de secuencia. No contiene credenciales.
- Rechazar formato/clave/algoritmo desconocidos, firma inválida, campos repetidos,
  exceso de tamaño y discrepancias con la entrega. Verificar antes de descargar
  y volver a verificar el plan transmitido al helper antes de ejecutar Setup.
- Mantener las comprobaciones de SHA-256 del archivo descargado y ejecutable:
  la firma autentica el manifiesto, no sustituye esas comprobaciones.

## Rotación, recuperación y compatibilidad

Una release de transición debe incorporar la clave pública inicial mediante un
canal de instalación cuya confianza confirme el responsable. Una clave descargada
sin autenticar desde el mismo repositorio no añade esa protección. Los clientes
anteriores no se vuelven verificadores de firmas por cambiar sólo el manifiesto.

Una rotación planificada publica primero un cliente que confía en la clave
actual y la siguiente, autorizado por la clave actual. Después cambia el firmador
y finalmente retira la clave anterior. Conservar el mayor estado de versión y
secuencia aceptados por instalación, separado de borradores y cuentas; una
respuesta de red no debe rebajarlo. Resolver copias de seguridad y reinstalación
antes de tratar ese estado como defensa ante rollback.

Si se pierde o compromete la clave, la recuperación requiere un instalador
confirmado por un canal independiente, o una clave de recuperación ya fijada.
No aceptar una revocación o una clave nueva sólo por provenir del repositorio
comprometido. Authenticode puede complementar este diseño cuando exista un
certificado y política de firma; no se ha supuesto ni generado uno aquí.

## Decisiones pendientes antes de implementarlo en producción

Responsable y ubicación de custodia, acceso al firmador, clave pública inicial,
canal de bootstrap, rotación y recuperación. Hasta resolverlas, no anunciar
«actualizaciones firmadas» ni generar una clave privada temporal como sustituto.
Las futuras pruebas con claves sintéticas deben ejecutarse por separado de la
publicación y sin claves o cuentas reales.