# Protocolo de contadores y baseline de 0.5.0

No se ejecutó el cliente ni se midió una sesión para esta entrega. La captura
aportada por el usuario (87,6 MB, 0,7 % CPU) y su rango 29–90 MB no identifican
la métrica exacta, el conjunto de procesos ni condiciones comparables. No se
atribuyen a una fuga ni se promete un porcentaje de ahorro.

La fuente base es bca0f0f895ad09f740066dd09dadd3dc6e564f32 más la corrección
local de límites de mensajes ya revisada. El artefacto publicado de referencia
es NotDiscord 0.4.1, ejecutable SHA-256
45f9035095d9ed4136c3c166ed4a0b1dc748ae69df7085ccdcf6043282e6febd.
Esa identidad no afirma que la instalación o el proceso actual del usuario sean
ese mismo archivo. El runtime preparado se inventaría por separado sin ejecutarlo.

## Helper optativo

scripts/measure-process-counters.ps1 recibe PID explícitos, una etiqueta de fase,
un archivo nuevo de salida y duración/intervalo acotados. No inicia NotDiscord,
no abre una cuenta, no se conecta a servicios, no lee contenidos de memoria ni
registra nombres, rutas, tokens, chats o títulos de ventanas. Los hijos son
optativos y se identifican por PID/parent/creación contrastada con StartTime del proceso abierto. Las identidades se fijan antes del muestreo, se rechazan PID reutilizados y se comprueba la relación temporal padre/hijo. Se revisó su sintaxis;
no se ejecutó para sortear las restricciones de pruebas del usuario.

Cada muestra JSONL registra UTC, fase, PID/parent/creación, working set, private
working set cuando EX2 esté disponible, private committed bytes, CPU acumulada
y normalizada, fallos de página, handles, hilos y bytes de I/O. Un dato ausente
es null. GPU y bytes de caché visual quedan null en este lector externo; sus
presupuestos del código no son una medición de residencia real. No sumar
working sets totales de varios procesos como memoria exclusiva.

## Comparación pendiente

Cuando se autorice o el usuario aporte mediciones de su uso normal, fijar antes
la duración y ciclos. Separar visible, segundo plano sin minimizar y minimizado;
repetir sin llamada y con multimedia, mismo contenido, misma app y procesos.
Comparar medianas/p95 por PID y con el mismo criterio de conjunto, distinguiendo
CPU/frame (requiere instrumentación adicional) de CPU por segundo. Correlacionar
sólo estados agregados de GIF/visor/voz y presupuestos de caché.

Working set menor con commit parecido y más fallos al regresar es compatible
con recorte de residencia de Windows. Commit y cachés que bajan juntos son
compatibles con liberación de recursos. Crecimiento repetido de commit al volver
al mismo estado necesita investigación; un pico aislado no demuestra fuga.
No se llama a EmptyWorkingSet ni se fuerza un objetivo artificial de 35 MB.