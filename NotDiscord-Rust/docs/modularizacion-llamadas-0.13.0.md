# Modularización y estabilidad de llamadas — ejecución 0.13.0

Trabajo iniciado el 9 de noviembre de 2026, America/Lima, mediante
`notdiscord-programar`, sobre `01c7ed88ae9bcdbe6b4b9288fe6c74fe2c641811`, rama
`NotDiscord`, variante estándar. Tipo inferido **Mediana: 0.12.0 → 0.13.0**,
por correcciones funcionales de llamada y contratos internos compartidos.
Se ejecuta el [plan vigente](plan-modularizacion-llamadas-conservador-2026-11-09.md).

## Evidencia y límites

El usuario confirmó que las desconexiones también ocurren en 0.12.0 y añadió:
«Como 15 minutos para arriba y siempre soy yo». Es evidencia reportada de cortes
prolongados que le afectan a él; no precisa aún qué transportes/capturas se
pierden ni prueba una causa del cliente, del servidor o de un temporizador.
No se introduce un plazo de desconexión de quince minutos como supuesto arreglo.

Continúan fuera de la ejecución el cliente con cuentas, llamadas reales,
dispositivos físicos, captura real, suspensión del PC e instalación. C5 y la
causa física de las desconexiones siguen abiertos. Fixtures, reloj virtual y
compilación se registran separadamente de cualquier uso real prolongado.
La preferencia permanente autoriza publicar al completar implementación y
controles permitidos; no autoriza instalar ni ampliar esas pruebas.

## Estado de implementación

T0–T7 están implementadas dentro de las comprobaciones permitidas; los casos
físicos de C5/H10 continúan abiertos. La entrega local T8 está generada y
verificada; la publicación se registra después de verificar el asset remoto.

Cambios integrados:

- T1: los detectores tick/lector del timeout de voz convergen en la misma entrada
  de fallo de transporte. Gateway conserva causa tipada, duración monotónica,
  edad del ACK y ocupación; la observación utiliza un slot del handle y el
  recorder, sin añadir eventos a la cola de entrega ni cambiar el presupuesto.
- T2: los callbacks de vídeo conservan el contexto del sink que los produjo.
  Suspender invalida el sink antiguo y reanudar crea uno con generación vigente.
  Main y Receiver comparten forwarding conservando su identidad y políticas.
- T3: preparación/configuración del host fuera del actor; plazo completo de
  escritura/respuesta y retención de resultados inciertos hasta salida real.
  Presupuestos de eventos y telemetría desde el host hasta el consumo de
  Completed; los permisos en vuelo permanecen contabilizados. Son límites de
  trabajo/payload, no una medición de RAM total del proceso.
- T4: un ejecutor del visor, una lectura activa y una demanda pendiente
  reemplazable. Cancelar no libera la lectura aún viva. Las huellas de fallo
  sobreviven a cachés/TTL; una acción individual permite el reintento. Las
  colisiones del filtro fijo son capacidad diferida local y no un error remoto.
- T5: descriptor común de superficies, cierre exterior de `@time`, foco y
  prioridad del popup hijo; la barrera estática ya no solicita otro frame a 16 ms.
- T6: proyecciones nativa/Gateway de vídeo separadas a partir de un modelo
  validado. H11 se difiere: un helper no aporta reducción material que compense
  mezclar las rutas JSON y multipart y sus límites distintos.
- T7: observación del primer ticket de ledger pendiente tras cinco segundos;
  es un umbral de diagnóstico, no un timeout de callback ni permiso de retry.
  C5/H10 permanecen abiertos. Las pruebas prolongadas ejecutadas utilizan reloj
  virtual y no acreditan llamadas físicas de varias horas.

## Comprobaciones ejecutadas

| Comprobación | Resultado |
|---|---|
| T0 | 179/179 archivos Rust de `src` caracterizados, sin faltantes; 11 contratos auxiliares revisados |
| Check integrado, todos los destinos y `native-probe` | Correcto, 20,66 s |
| Librerías del workspace | **1.021 correctas**, 0 fallidas; app 784, core 178, media 59; 5 ignoradas existentes |
| Addon diagnóstico | **61 correctas**, 0 fallidas; solapadas con las 59 de media |
| Drivers N-API con fakes | Dos correctos; 8 ciclos de publicación y 64 de receptor, sin motor externo, red ni dispositivos |
| Heartbeat y vídeo | Tick/lector equivalentes; callback RGBA vigente activa con generación correcta y el sink antiguo se rechaza |
| Salida de publicación en la matriz | 32 fases y 160 eventos consumidos; controles de parada exigidos, sin ocultar saturación |
| Visor y fallos | Worker real con lector falso bloqueado, cien sustituciones coalescidas, lease retenido hasta lectura final; fallos/TTL/evicción no admiten nuevos intentos |
| Prolongación sintética | Cuatro horas virtuales y 14.400 frames del modelo con tickets de rekey simulados; sin ejecución de rekeys nativos ni llamada real |
| Build de producción | Correcto, release sin `native-probe`, 6 min 25 s |
| Check release de todos los destinos | Correcto, 29,33 s |
| Paquete y runtime | 41 archivos, inventario/allowlist/licencias y hashes correctos; 31 archivos runtime preparados sin ejecutar |
| Exportaciones de producción | Protocolo1; protocolVersion, run, sanitizeStats, validateEndpoint; sin probes |
| IPC del paquete | 7 correctas, salida0, ninguna etapa de inicialización nativa ni activación de dispositivos |
| Setup | Generado, no ejecutado; metadatos pendientes coinciden con Setup y ejecutable |

El primer check detectó seis errores de visibilidad/API en fixtures de recursos,
corregidos antes del check integrado correcto. La primera suite tuvo 783 pruebas
de app correctas y una fixture antigua que esperaba un error síncrono de apertura.
Se ajustó para consumir la finalización real `prepared`, sin retirar las guardas
ni las aserciones de error visible/limpieza. La fixture focal y la suite completa
pasaron después. No se ignoró ninguna prueba para resolver esos fallos.

El primer driver N-API acumulaba las fases de su matriz sin consumir salida,
apoyándose en la cola ilimitada anterior. Se corrigió únicamente la fixture
`receive_probe.rs`, excluida de producción por `cfg(native-probe)`: consume la
salida entre fases, rechaza cualquier `fatal` y exige cada parada. Después
pasaron de nuevo las 61 pruebas diagnósticas y ambos drivers. Las fuentes de
producción permanecieron iguales a las validadas por la suite completa.

Los cortes de tests y producción se distinguen en
[`source-cut-0.13.0-tests.json`](../evidence/source-cut-0.13.0-tests.json) y
[`source-cut-0.13.0-delivery.json`](../evidence/source-cut-0.13.0-delivery.json).
El [informe T0](../evidence/t0-review-0.13.0.json) conserva procedencia, límites y
huellas de revisión separados de las huellas finales. El build ID final es
`61bc8d3708e4dc9fed61448e6039e1da75ede2eea6079f2d7c5bd9800137637b`.

Los contratos se registraron por separado: `413ef27f` para voz/IPC y
`abf5704a` para recursos/superficies, sobre el corte integrado comprobado. El
commit de entrega posterior fija el manifiesto compilado `release: null` y las
evidencias; el anuncio remoto del instalador sólo se hará después de verificar
el asset.

## Entrega local

| Artefacto | Bytes | SHA-256 |
|---|---:|---|
| `NotDiscord-Setup-0.13.0.exe` | 119119054 | `a595cfad04df656a18924948637e94eca28bf3a5a4f9d5d7012a1aa4e6f291b9` |
| `NotDiscord.exe` del payload | 28647936 | `8d197a2e18d7b9eedd1c5b40f048cce77afd59f9f248be9a3433f9cfb7401cdd` |
| `NotDiscord.Media.node` | 4467712 | `c7229ef7b8258ab91ef2cbc9c05f7222604740bd380f9d8010a2d424ebbeee7f` |

El [acta local](../evidence/delivery-0.13.0-local.json) fija fuentes, logs y
artefactos y conserva el estado anterior a la publicación. El build ID fue
encontrado dentro del ejecutable empaquetado. La comprobación final contrastó
las 189 entradas del corte y preservó Setup0.12.0, el informe IPC anterior y los
seis archivos ajenos al alcance. Se conserva el enlace al plan que el usuario
había añadido a README.

Los probes N-API cargaron sólo nuestro addon con objetos falsos. La comprobación
IPC del paquete carga el módulo nativo para capacidades pero no inicializa el
motor ni dispositivos. No se abrió el cliente, no se usó cuenta Discord, no se
instaló ni ejecutó Setup. No se midió CPU/RAM/p95; las cuotas y los contadores
sintéticos no certifican ahorro de memoria ni una llamada física prolongada.

Se preservan los cambios anteriores del usuario en README, plan del 31 de
octubre y skill de planificación, además de la evidencia ajena. No se modifican
Plus/legacy, datos, sesión, configuración, dependencias ni política de recuperación
para ocultar síntomas. Las hipótesis H10/C5 no justifican un timeout nativo nuevo,
ACK anticipados ni reintentos adicionales.
