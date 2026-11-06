---
name: notdiscord-programar
description: "Implementa, corrige y entrega NotDiscord o NotDiscord+ en Rust, siguiendo un pedido o plan vigente. Úsala para programación, continuar una tanda o preparar una versión; las solicitudes exclusivamente de análisis o planificación se resuelven sin modificar el producto."
---

# Programación de NotDiscord

Convierte el pedido o plan vigente en cambios revisables, manteniendo las
funciones existentes y la variante elegida. Una corrección pequeña no requiere
un plan global nuevo. Si el pedido incluye analizar y aplicar, realiza ambos
pasos en la misma tarea sin pedir aprobación adicional de la implementación ya
autorizada. Planificar por sí solo no autoriza programar ni publicar.

## Alcance y versión

Localiza el repositorio y su checkout; revisa las instrucciones aplicables, los
cambios locales y los mensajes posteriores al plan. Identifica variante, rama,
restricciones de cuentas, ejecución y pruebas, y entregables autorizados. Aplica
la preferencia permanente de publicación descrita en Entrega sin extenderla a
otra variante, cuentas o instalación. Conserva los cambios del usuario y los
datos fuera del alcance.

Determina siempre el tipo de actualización antes de fijar su versión:

| Tipo | Criterio del usuario | Incremento desde `x.y.z` |
|---|---|---|
| Pequeña | Fix menor, rápido y acotado | `x.y.(z + 1)` |
| Mediana | Error mayor, como un crasheo, o pequeñas adiciones | `x.(y + 1).0` |
| Grande | Corrección de alta prioridad: seguridad, vulnerabilidad o riesgo fundamentado de detección/autobaneo | `(x + 1).0.0` |

Respeta una elección explícita del usuario. Si no indicó el tipo y es claramente
inferible, comunica la clasificación y el motivo y continúa. Si no está claro,
pregunta **Grande, Mediana o Pequeña** antes de versionar, mientras avanzas en lo
independiente. No pidas confirmación redundante cuando ya eligió o la inferencia
es clara. Registra tipo, origen, justificación y versión nueva. Si cambia el
alcance, reconsidera la clasificación respetando la elección vigente. No
renumeres versiones publicadas ni prometas evitar bloqueos de Discord.

## Implementación y comprobación

Lee [el flujo del proyecto](references/project-workflow.md) para ubicar contratos
y componentes. Sigue la causa hasta sus efectos en las vistas implicadas y
reutiliza la lógica compartida. Para investigar artefactos de otro cliente con
REA, usa la skill de ingeniería inversa disponible; no la necesitas para la
arquitectura ordinaria de nuestro repositorio.

Divide cambios con dependencias reales y delega trabajo independiente con
propiedad de archivos explícita. Integra las fuentes antes de compilar. Mantén
cancelación, permisos, selección, anclajes, persistencia y conexiones que el
cambio afecte. No reduzcas funciones operativas para ocultar un síntoma.

Verifica dentro de los límites actuales. Una prohibición de pruebas funcionales
incluye iniciar cliente, cuenta, llamadas, dispositivos o instalador para
comprobar comportamiento; no se elude mediante una instancia supuestamente
aislada. Revisión estática y compilación no equivalen a funcionamiento probado.
Cuando esas restricciones no existan, elige comprobaciones proporcionadas al
cambio; no añadas tests que sólo repitan la implementación.

Documenta evidencia, decisiones y cambios materiales del plan. Para detalles
rutinarios procede sin preguntar; pide información sólo si falta una decisión
que afecte al alcance o a una acción aún no autorizada.

## Entrega

**Preferencia permanente del usuario:** al terminar la implementación de un plan
y sus comprobaciones permitidas, publica en GitHub las fuentes pertinentes, el
instalador y el manifiesto de actualización de la variante y versión acordadas,
para actualizaciones **Pequeñas, Medianas y Grandes**. Incluye esta publicación
en la ejecución del plan, sin pedir otra confirmación ni detenerte en una entrega
local. Una instrucción posterior de no publicar o limitarse a entrega local
prevalece. Redactar un plan o modificar sólo documentación no publica el producto.

Sigue [el flujo de entrega](references/release.md): verifica el asset antes de
anunciar el manifiesto. Los ensayos de cuenta, dispositivos o suspensión real
no autorizados se documentan como pendientes y no bloquean por sí solos una
publicación cuyos controles permitidos pasaron; no anuncies esos casos como
corregidos o probados. Un fallo de compilación o de una comprobación requerida
debe resolverse antes de publicar. La publicación no autoriza instalar, iniciar
el cliente, acceder a cuentas ni usar dispositivos.

Para una entrega **Pequeña** autorizada, genera y publica el instalador y su
manifiesto; deja que el usuario actualice la instalación desde NotDiscord.
Conserva la instalación, los accesos directos y las otras copias de uso del
usuario salvo que pida actualizarlos. Los binarios temporales de compilación y
empaquetado siguen siendo necesarios para producir el instalador. Fuera de la
ejecución de planes y de las entregas Pequeñas, resuelve la publicación según el
pedido vigente.

Entrega la variante y versión acordadas e indica causa, corrección, evidencia,
comprobaciones y limitaciones. Distingue lo implementado de lo verificado y de
lo pendiente de confirmación del usuario. Actualiza el estado de la tanda para
poder continuarla sin repetir trabajo terminado.
