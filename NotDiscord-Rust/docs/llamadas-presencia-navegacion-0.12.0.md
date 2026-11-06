# Llamadas, diagnóstico y recuperación — 0.12.0

**Estado actual:** publicada posteriormente por petición explícita del usuario
como [NotDiscord 0.12.0](https://github.com/SlimoDev/RustyCord/releases/tag/NotDiscord-v0.12.0).
El [acta de publicación](../evidence/publication-0.12.0.json) registra la verificación.
Los siguientes párrafos conservan el corte local previo a esa publicación.

Corte de entrega local de NotDiscord estándar, rama `NotDiscord`, del 6 de noviembre de
2026. Actualización **Mediana**, inferida por las correcciones funcionales y la
recuperación de voz: 0.11.0 → 0.12.0. Base de trabajo
`3bee4c8c6a9e7442817302850286d170c56976ad`; fuentes con cambios locales sin commit.
La publicación anterior continúa siendo 0.11.0. El manifiesto de esta compilación
mantiene `release: null`.

## Cambios del plan

- **T7 / C1:** publicación remota, intención de visualización y decoder tienen
  estados distintos. Un fin confirmado retira sólo la pantalla y emite un único
  sonido semántico por publicación. Cerrar la visualización conserva la oferta
  remota. Los fallos del receptor seleccionado permiten una recuperación dentro
  del episodio compartido; al agotarse dejan un aviso sin tarjeta de error ni
  control inoperante. Los cierres conservan dueño hasta la respuesta nativa,
  terminación del socket y confirmaciones pertinentes, también con ACK tardío o
  una nueva publicación que reutiliza la clave. La parada propia publica el
  estado local sin esperar la escritura Gateway en el actor.
- **T8 / C5:** observación local voluntaria de hasta diez segundos, diez consultas
  y una consulta por segundo. Correlaciona intención VAD/PTT, puerta aplicada,
  cifrado, antigüedad del callback y deltas de audio de la conexión principal.
  Ausencias, reinicios de contadores y cambios de cifrado conservan su significado;
  no se rellenan con ceros que aparenten transmisión. No graba audio, no crea
  polling permanente y no incorpora nombres de dispositivos ni datos de cuenta.
  Una consulta pendiente conserva su lease después del timeout o cancelación.
  Cada host tiene identidad diagnóstica propia para rechazar resultados antiguos.
- **T9 / C3:** episodio compuesto Gateway → voz → visualización, con una sola
  etapa en curso y un intento por etapa necesaria. La reanudación de voz conserva
  conexión y DAVE cuando siguen siendo válidos, usa Resume/op7 con `seq_ack` de
  frames realmente aplicados y espera op9/cifrado antes de transmitir. Los códigos
  terminales detienen la serie; una sesión inválida puede seleccionar Fresh antes
  de emitir, sin encadenar un Identify tras fallar Resume. Fresh libera primero
  los recursos locales, incluso con Gateway sin sincronizar y la sesión principal
  ya retirada. La intención de llamada y las selecciones explícitas se conservan.
  Las notificaciones tipadas de energía de Windows se coalescen; no se infiere
  suspensión por un salto del reloj ni se suspende realmente el PC en las pruebas.
- **T10 / C4:** volver del perfil utiliza el snapshot real del frame y mantiene
  cuenta, generación y origen de navegación.
- **T11 / C2:** los registros diferencian actor y persona retirada por sus IDs;
  los datos incompletos producen texto neutral. Las expectativas de las fixtures
  son literales e independientes del helper de producción.
- **T12 / C6:** el área del usuario y la galería comparten el predicado de speaking,
  con las mismas guardas de llamada, identidad, cifrado, mute y deaf.
- **T13 / C7:** revisión por archivo y responsabilidad de los tres crates. Los
  cambios adicionales justificados corrigen contexto de generación en GIFs,
  selección de presencia desde el perfil, sonidos entre intentos y el lease de
  preview. Un timeout de preview ya no permite otra consulta mientras sigue viva
  la anterior; un callback antiguo no entrega su imagen ni libera otra consulta.
  El error E0425 de `preview.rs` procedía de argumentos declarados en la rama
  equivocada y queda corregido junto con la comprobación de ambas rutas.

No se cambió `streamParameters` bajo la hipótesis descartada de sustitución total.
No se modificaron dependencias, datos, configuración ni cuotas generales como
supuesto arreglo. Se conservan operaciones de envío inciertas, rate limits,
permisos, borradores y separación entre variantes.

## Comprobaciones y artefactos

| Comprobación | Resultado |
|---|---|
| Librerías del workspace | **995 correctas**, 0 fallidas; 5 ignoradas existentes: app 766, core 176, media 53 |
| Addon con `native-probe` | **55 correctas**, 0 fallidas; se solapan con las 53 de media |
| N-API con objetos falsos | Continuidad: 8 ciclos de cámara/pantalla/micrófono y 64 ciclos de receptor; ambas API de preview, PNG/RGBA, callbacks tardíos y cierre de GoLive sin main verificados |
| Revisión T13 | **174/174** archivos Rust de `src`, sin faltantes; tres responsabilidades compartidas se cuentan una sola vez |
| Runtime local | 31 archivos inventariados y copiados con hash; referencia conservada y no ejecutada al prepararla |
| Build de producción | Correcto, workspace release sin features diagnósticas; 7 min 17 s |
| Check release de todos los destinos | Correcto, 34,40 s |
| Paquete | 41 archivos; allowlist, manifiesto, licencias e inventario verificados |
| Exportaciones de producción | Protocolo 1; sólo protocolVersion, run, sanitizeStats y validateEndpoint |
| IPC del paquete | **7 comprobaciones correctas**, salida 0, ninguna etapa de inicialización nativa ni activación de dispositivos |
| Instalador | Generado y contrastado con los metadatos pendientes; no ejecutado |

La primera ejecución restringida produjo 761 pruebas correctas y cinco fallos de
infraestructura: acceso denegado a pipes privados y creación de junctions. Se
repitió la suite sin ese token restringido y pasó completa, sin omitir ni rebajar
esas pruebas. La compilación diagnóstica detectó después una variable de sesión
fake que ocultaba el receptor `mpsc`; se corrigió su nombre. Ese archivo sólo
compila con `native-probe`: sus 55 pruebas y los dos drivers pasaron después de
la corrección, sin modificar las fuentes de producción validadas por la suite.

Los cortes y la revisión están en
[`source-cut-0.12.0-delivery.json`](../evidence/source-cut-0.12.0-delivery.json),
[`source-cut-0.12.0-workspace-tests.json`](../evidence/source-cut-0.12.0-workspace-tests.json)
y [`t13-review-0.12.0.json`](../evidence/t13-review-0.12.0.json).
El corte de producción contiene 184 entradas, incluidas 174 fuentes Rust de
`src` y el `build.rs` del addon. Identifica los bytes de trabajo exactos, no los
blobs que Git podría normalizar al crear un commit. Las dependencias/vendor y
los activos no modificados se conservan desde la revisión base.

Build ID de producción (SHA-256 del corte final):
`b953c4e22ef2384c5424917ac28f94479e7411910b25e7ecefaa1713fba4691e`.
El mismo identificador se encontró en el ejecutable empaquetado. Las 184 entradas
del corte conservaron sus hashes después de generar Setup. El paquete se volvió
a verificar tras la comprobación IPC y la compilación del instalador.

| Artefacto | Bytes | SHA-256 |
|---|---:|---|
| `dist/NotDiscord-Setup-0.12.0.exe` | 119095983 | `5097b8b7d034ffb5c5b7e8786c5984a0654c41f6b3bf855aa063f632f8874915` |
| `NotDiscord.exe` del payload | 28555264 | `a58cee23c11e59dd91bd3cab39b9e007f5a5ffea9359601c2243bb1e35b821e0` |
| `NotDiscord.Media.node` de producción | 4454912 | `5671a71f3af9592306bb2f0f09250e3b64c2550aceff953d7b72a87415eacdde` |

El [acta de entrega local](../evidence/delivery-0.12.0-local.json) contiene las
huellas de artefactos, fuentes y logs, el resultado de cada comprobación y las
referencias conservadas de Plus/legacy. Setup anterior 0.11.0, los seis archivos
ajenos al alcance y el informe IPC previo conservaron sus bytes. Las fuentes de
esta ronda quedan sin commit para su revisión; no se hizo push ni release.

[`dist/version-0.12.0.pending.json`](../dist/version-0.12.0.pending.json) está
preparado para una eventual publicación. Su URL futura no está verificada y no
se anuncia desde `version.json`. El [Setup local](../dist/NotDiscord-Setup-0.12.0.exe)
es el entregable generado; el payload es sólo un temporal de compilación.

## Límites pendientes

**C5 continúa abierto:** las pruebas sintéticas y los contadores locales no
acreditan audibilidad remota ni identifican por sí solos la causa física del
micrófono. No se ejecutó el cliente, se accedió a cuentas, se abrieron dispositivos,
se capturó una pantalla real, se suspendió el PC ni se ejecutó Setup.

Los tests de protocolo utilizan fixtures y, cuando corresponde, sockets de
loopback. Los probes N-API de esta ronda cargan únicamente nuestro addon con
objetos JavaScript falsos; no cargan el motor multimedia de terceros. La prueba
IPC del paquete carga el bootstrap y el módulo nativo para capabilities, y se
limita a capabilities/stats/shutdown sin inicializar el motor ni dispositivos.
El funcionamiento con Discord real, cámara/micrófono y suspensión real permanece
sin comprobar. La revisión T13 no certifica ausencia de fallos ni auditoría de
dependencias/vendor; no se midieron CPU, RAM, working set ni p95 de frame y no se
anuncia un ahorro de rendimiento.

El corte local anterior terminó con el instalador sin publicar ni ejecutar.
Después el usuario pidió subir 0.12.0 y mantener esa publicación como paso normal
al finalizar los planes. Se publicaron las fuentes en
`9eb222ffd3a3e962106864317c6cb0a674f13209`, la release con título exacto
`NotDiscord 0.12.0` y el asset; nombre, estado, tamaño y digest coinciden con Setup.
El enlace público respondió HTTP 200 y la etiqueta apunta al commit de fuentes.
El manifiesto se anuncia después de esas verificaciones, conservando `release: null`
en el commit de fuentes y dentro de Setup. No se instaló ni sustituyó la
instalación del usuario. NotDiscord+ y `legacy` quedan fuera de esta entrega.
