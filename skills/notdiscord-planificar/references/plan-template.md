# Contrato para continuar la implementación

Adapta la longitud al pedido. Una corrección pequeña no requiere rellenar una
auditoría completa; conserva los campos que cambian la decisión o su ejecución.

## Identificación y alcance

- Objetivo, fecha, variante, rama, revisión base y cambios locales relevantes.
- Evidencia del usuario y comportamiento solicitado; funciones que conservar.
- Límites de la tarea y restricciones vigentes, con su origen.
- Tipo de actualización y propuesta de versión cuando corresponda, con
  justificación según la política vigente y sin modificar el manifiesto.

## Evidencia y prioridades

Para cada hallazgo: prioridad, archivo/función/líneas, causa, precondiciones,
impacto, grado de certeza, mitigaciones existentes y propuesta. Indica si el
resultado es revisión estática, medición, reproducción o una hipótesis. En
seguridad no asignes severidad alta sólo por el tema; en rendimiento no
confundas presupuestos internos con memoria total o ahorro medido.

## Tandas y aceptación

Por tanda, define objetivo, dependencias, contratos y superficies compartidas,
propiedad de archivos si se delega y comportamiento verificable. Preserva
selección, scroll, permisos, cuentas, cancelación, red y datos cuando se vean
afectados. Incluye migración, respaldos y recuperación sólo cuando correspondan.

## Comprobaciones y entrega

Distingue lo que ya se comprobó de lo propuesto. Indica qué puede verificarse
de forma estática, con compilación o pruebas puras y qué sigue limitado por las
restricciones de ejecución/cuentas. No conviertas un probe funcional en prueba
permitida cambiándole el nombre o el directorio de datos.

Registra entregables y destinos autorizados: un plan no implica instalador,
publicación ni cambios del producto. Mantén estado de tandas, trabajo pendiente,
decisiones abiertas y hechos que se deben revalidar al continuar. Si el pedido
ya autoriza implementar, la aprobación del plan no es un requisito adicional.
