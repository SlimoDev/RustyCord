# Entrega local de NotDiscord 0.11.0

4 de noviembre de 2026. Variante estándar, rama `NotDiscord`, base `865b2f11069fadcd40f40bc634e7162ea7694f3e`, con cambios locales sin commit. Continuación del pedido «Ejecuta todo con la skill de notdiscord-programar». Actualización **Mediana**, inferida por las correcciones funcionales y pequeñas adiciones; se conserva 0.11.0 porque esta versión aún no se ha publicado.

## Alcance implementado

- T1: descubrimiento dirigido de llamadas privadas con op13, roster y autoridad de respuestas sin nonce, proyección inmediata de salida local y cierre con propietario y confirmaciones correlacionadas. No hay polling, ring, reenvío automático ni bloqueo de navegación por fallo del descubrimiento opcional.
- T3: presencia visual inmediata separada del estado confirmado, una escritura activa, última elección coalescida y tratamiento explícito de rechazo, 429 y resultado incierto.
- T4: cabecera de vídeo ampliado en una fila con icono Miembros, Volver y Ajustar; adaptación a ventanas estrechas.
- T5: menú del registro RAM y regreso al canal de origen, conservando permisos, caché válida, borrador y contexto de navegación.
- T2: diagnóstico saneado, matriz de dobles, análisis del motor y prueba nativa de continuidad sintética. **No se ha reproducido ni corregido de forma acreditada la pérdida del micrófono físico.**

La [nota de implementación](llamadas-presencia-navegacion-0.11.0.md), el [plan actualizado](plan-llamadas-presencia-navegacion-2026-11-04.md#8-ejecución-posterior-autorizada--4-de-noviembre-de-2026) y la [nota del micrófono](microfono-0.11.0.md) contienen las decisiones y límites. Se conservaron los cambios ajenos y las otras variantes.

## Fuentes y producción

`evidence/source-cut-0.11.0-delivery.json` registra 32 archivos modificados/nuevos de producto, drivers y manifiestos. Los demás archivos de la base se identifican por la revisión Git. Se contrastaron los hashes antes y después de compilar y empaquetar. Su SHA-256 se pasó como `NOTDISCORD_BUILD_ID` y se encontró en el ejecutable sin iniciarlo:

```text
5b6286f2043eab9772d2ddd68352bf044f4d4d3ee296f4647b52b684ffba766c
```

Estas huellas describen las entradas de compilación del directorio de trabajo Windows, que incluye finales CRLF/LF mixtos. El inventario y las evidencias seleccionadas se conservan byte por byte mediante atributos específicos. El código y Cargo mantienen la normalización habitual de Git: `evidence/source-git-map-0.11.0.json` relaciona las 32 entradas de compilación con el identificador y SHA-256 de su blob Git, comprobado contra el contenido normalizado por el checkout. No se regeneró el inventario ni se atribuye al binario el manifiesto de publicación añadido después del asset.

Comandos de producción, mediante el wrapper MSVC x64 del repositorio:

```powershell
cargo build --workspace --release --locked --offline --jobs 2
cargo check --workspace --all-targets --release --locked --offline --jobs 2
```

Ambos finalizaron correctamente: build en 5 min 51 s y check en 20,01 s. Se conservaron las advertencias existentes. Los binarios de producción usan features predeterminadas; el addon exporta únicamente `protocolVersion`, `run`, `sanitizeStats` y `validateEndpoint`. No contiene entrypoints de probes. Su protocolo es 1.

| Componente | SHA-256 |
|---|---|
| `NotDiscord.exe` | `6973d523f65f255505c56d1e7d6ae1670471e8d6fd50082143d6bc8df38ade54` |
| `NotDiscord.Media.node` | `509d4df50592af0ea36ef5f7c137b2e7b90d6acd0fdfc4db1b77faf20630eac2` |
| Motor externo `discord_voice.node` | `7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607` |

El runtime se preparó en `target/prepared-runtime-0.11.0-20261104-full`, con 31 archivos inventariados y hashes fijados. El payload nuevo `target/installer-payload-0.11.0-20261104-full` tiene 41 archivos y pasó `verify-package.ps1`. No contiene datos de cuentas ni cachés; es un temporal de compilación, no una distribución portable. Los paquetes previos se conservaron.

## Instalador generado

[NotDiscord-Setup-0.11.0.exe](../dist/NotDiscord-Setup-0.11.0.exe), **118951901 bytes**. SHA-256:

```text
23024f01534d4a886ec4a26453cd046832f272348f77352c2fae4093e7012fee
```

Se generó mediante `build-installer.ps1` e Inno Setup existente. El informe `evidence/delivery-0.11.0-local.json` relaciona instalador, ejecutable, addon, runtime e inventario de fuentes. `prepare-update-release.ps1` preparó `dist/version-0.11.0.pending.json` con tamaño y hashes locales. **Es un borrador, no un anuncio de actualización:** su URL/tag remotos no se han publicado ni verificado. El manifiesto raíz y el incluido en Setup mantienen `release: null`, evitando anunciar un asset inexistente y la autorreferencia del hash del instalador.

## Comprobaciones

| Comprobación | Resultado |
|---|---|
| Tests de workspace, librerías, dependencias bloqueadas/offline | **939 correctos**, cero fallos; app 731 y cinco omitidos, core 169, media 39. |
| Tests del addon con `native-probe` | **41 correctos**, incluidos dos de política/correlación del probe. Se solapan con los 39 de media. |
| ABI nativa estática | Los deltas de vídeo no eliminan audio por omisión. Flags fake seguidos hasta las fábricas FakeADM/Strobe del hash inspeccionado. |
| Motor real con FakeADM y UDP localhost | Captura sintética y contadores nativos de paquetes progresaron en cinco fases de descriptor/OwnStream; liberación final correcta. No hubo peer DAVE ni escucha remota. |
| Probe de vídeo ficticio | **Falló** con timeout y cero frames. Límites de identidad del fixture y selección Strobe/Y4M documentados; no acredita un fallo de cámara del producto. |
| Addon de producción cargado en Node | Protocolo 1, sólo cuatro exports esperados, sin cargar el motor externo. |
| Host de producción: capabilities/stats/shutdown | **7 comprobaciones correctas**, salida 0; cero inicializaciones nativas, llamadas o dispositivos activados. |
| Formato y huellas | Diff sin errores de espacios; CRLF permitido por el checkout. Fuentes del corte sin cambios durante la entrega. |

Evidencias actuales: `t6-package-0110.json`, `t6-production-exports-0110.json`, `t6-production-host-0110.json`, `t2-native-execution-0110.json` y sus reportes enlazados en `evidence/`. El inventario nativo incluye el hash del addon diagnóstico, distinto del addon de producción. Los resultados del primer corte siguen conservados como históricos.

Los logs están en `.tools/temp/current/` del repositorio: `calls-0110-full-workspace-tests.log`, `calls-0110-native-unit-final.log`, `calls-0110-production-build.log`, `calls-0110-delivery-check.log`, `calls-0110-package.log`, `calls-0110-production-host.log` y `calls-0110-installer.log`.

## Estado de validación funcional

No se abrió la aplicación con una cuenta, no se accedió a credenciales y no se activaron micrófono, cámara ni captura de pantalla físicos. Los probes usaron entradas sintéticas; no son la matriz audible de R2. El instalador se compiló sin ejecutarlo. No se hizo commit, push, publicación ni actualización de la instalación o accesos directos del usuario; `version.json` conserva `release: null`.

Para completar R2 hace falta una llamada real autorizada: establecer voz base, iniciar/parar/cambiar una ventana de prueba y cámara, repetir VAD/PTT, correlacionar callbacks, puerta/cifrado y paquetes, y confirmar audibilidad desde otro participante. Ese ensayo no se realizó. El procedimiento propuesto no graba audio/vídeo ni envía invitaciones. La prueba debe discriminar una pérdida de captura, puerta, publicación o recepción antes de decidir una corrección. También siguen pendientes la comprobación del descubrimiento/presencia desde otro cliente y la revisión visual manual a distintos DPI.
