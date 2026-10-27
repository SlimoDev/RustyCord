---
name: notdiscord-planificar
description: "Analiza NotDiscord o NotDiscord+ en Rust y prepara un plan técnico con evidencia, prioridades y tandas verificables. Úsala para solicitudes de planificación, auditoría de rendimiento o seguridad, o investigación previa; una corrección directa pertenece a programación y no requiere un plan global."
---

# Planificación de NotDiscord

Convierte el pedido en un plan ejecutable basado en la revisión vigente del
proyecto. Planificar por sí solo no autoriza modificar el producto, compilarlo,
ejecutar el cliente o dispositivos, utilizar cuentas, instalar ni publicar.
Si el usuario pide analizar y aplicar, prepara un plan proporcional y continúa
la implementación ya autorizada con la skill de programación disponible; no
inventes una aprobación intermedia.

## Situar el análisis

Localiza el repositorio mediante el contexto y Git; lee instrucciones aplicables,
estado local, variante y documentación pertinente. Usa el
[mapa del proyecto](references/project-map.md) para seguir componentes compartidos.
No dependas de rutas de otro equipo ni cambies de rama para planificar. Conserva
los cambios locales, las otras variantes y las restricciones vigentes de cuentas,
ejecución, dispositivos, comprobaciones y publicación; no generalices una
restricción de una sesión a todos los pedidos futuros.

Una cuenta, el producto instalado y los datos privados no son necesarios para
una revisión ordinaria de fuentes. Si hay evidencia de usuario, distingue lo
observado por él de lo medido por el agente. Para estudiar otro cliente o un
artefacto compilado, usa la skill de ingeniería inversa disponible cuando esa
evidencia sea necesaria; no hace falta REA para la arquitectura de estas fuentes.

## Fundamentar las decisiones

Sigue cada causa por sus callers y efectos, revisando límites y mitigaciones
existentes. Cita función y archivo con líneas de la revisión observada; registra
hash/revisión si hay cambios concurrentes. Separa fallos confirmados, hipótesis,
endurecimiento y decisiones de diseño. La complejidad, un `unwrap` o una cola
por sí solos no prueban un fallo alcanzable.

Si cambian fuentes durante el análisis, revalida los hallazgos afectados antes
de entregar y explicita el corte observado.

En rendimiento, diferencia CPU medida de trabajo inferido y cachés parciales
de RAM del proceso. Distingue working set, memoria comprometida, GPU y procesos
auxiliares; una bajada de RAM o un pico no demuestran una liberación o fuga.
Propón mediciones comparables antes de prometer ahorros. En seguridad, explicita
precondiciones, superficie, mitigaciones y alcance; no copies secretos ni uses
dumps de cuentas reales. Verifica avisos cambiantes con fuentes primarias y el
lock/target/features efectivos, sin presentar un muestreo como auditoría total.

Prioriza causas compartidas y eliminación de trabajo redundante antes que
duplicar lógica o reducir funciones para ocultar síntomas. Propón tandas con
dependencias reales, invariantes, criterios de aceptación, recuperación y
comprobaciones permitidas. Las comprobaciones propuestas no son resultados.

## Entregar y poder continuar

Usa el [contrato de plan](references/plan-template.md), adaptándolo al alcance.
Entrega en el chat o guarda el archivo según el pedido. Si pide un archivo del
proyecto sin destino, sigue la documentación existente de su checkout. Actualiza
un plan pertinente antes de crear duplicados. Mantén el estado de cada tanda y
las decisiones posteriores; una revisión futura debe revalidar una base que cambió.

Para versión y entregables, consulta la política vigente del proyecto. No
renumeres versiones publicadas ni heredes la versión de otra variante. Registra
una propuesta justificada cuando corresponda, sin modificar el manifiesto por
planificar. No prometas evitar bloqueos de Discord.

Informa la conclusión principal, enlaza el plan cuando exista archivo y distingue
trabajo terminado, implementación pendiente y dudas que realmente dependan del
usuario. Continúa el análisis independiente mientras se aclara una decisión de alcance.
