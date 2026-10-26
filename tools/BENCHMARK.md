# Medición de RAM y CPU

Ejecuta el launcher y espera 60 segundos para estabilizarlo. Identifica el PID del proceso principal en el Administrador de tareas, pestaña Detalles. La herramienta también detecta la raíz automáticamente cuando existe una sola candidata.

Ejemplo desde PowerShell:

~~~powershell
& 'E:\DESCARGAS\REA\NotDiscord\tools\Measure-Client.ps1' -Client NotDiscord -Scenario idle-visible -DurationSeconds 45
& 'E:\DESCARGAS\REA\NotDiscord\tools\Measure-Client.ps1' -Client Discord -RootProcessId 1234 -Scenario idle-visible -DurationSeconds 45 -Notes 'Mismo canal, ventana visible, sin llamada'
~~~

Sustituye 1234 por el PID real del proceso principal. Para seleccionar una raíz explícita utiliza siempre -RootProcessId; no selecciones un renderer WebView2 ni el actualizador. El medidor no abre, cierra ni cambia la aplicación.

Se generan CSV y JSON en evidence\benchmarks, con una muestra por segundo, todos los PID observados, memoria privada, working set y CPU. Puedes cambiar -OutputDirectory, -IntervalSeconds y -DurationSeconds. Las sesiones de comparación recomendadas duran entre 30 y 60 segundos.

La selección usa Toolhelp32 y recorre únicamente los descendientes de la raíz elegida. No necesita CIM/WMI y no suma todos los procesos msedgewebview2 del equipo. El PID y la hora de inicio identifican cada proceso. Si la raíz termina o su PID se reutiliza, el JSON marca la sesión incompleta.

CPU se calcula como el incremento de tiempo de CPU dividido entre el intervalo real y los procesadores lógicos activos del sistema. Por ejemplo, un núcleo ocupado en un equipo de ocho procesadores lógicos representa aproximadamente 12,5 %. CpuCoreEquivalents permite ver el consumo equivalente en núcleos. La primera muestra establece una referencia; un proceso que aparece después también necesita su primera referencia.

PrivateMiB representa memoria privada comprometida. WorkingSetMiB representa memoria residente y puede contar páginas compartidas más de una vez al sumar procesos. No son métricas intercambiables ni representan por sí solas el ahorro total del sistema. El medidor no mide GPU. Los procesos muy breves y el último intervalo de CPU de procesos que terminan pueden quedar fuera del muestreo.

Para comparar:

1. Usa el mismo canal, tamaño de ventana, cuenta, plugins, dispositivos y actividad.
2. Mide por separado reposo visible, reposo minimizado, navegación, voz y vídeo. Espera a que termine la carga inicial.
3. Alterna el orden de los clientes y repite tres veces cada escenario.
4. Compara medianas de memoria privada, working set y CPU media normalizada. Conserva los JSON originales y las notas de cada ejecución.
5. Una medición de Discord ya abierto con actividad desconocida es una referencia descriptiva; no demuestra ahorro frente al prototipo.

Prueba del medidor:

~~~powershell
& 'E:\DESCARGAS\REA\NotDiscord\tools\Test-Measure-Client.ps1'
~~~

La prueba crea una raíz PowerShell, un hijo con trabajo de CPU y un proceso ajeno. Todos terminan por sí solos, sin terminar aplicaciones del usuario. Verifica descendencia, exclusión del proceso ajeno, memoria, fórmula CPU y una cadena sintética de dos generaciones. Guarda resultados en tools\test-output. Esos resultados sintéticos no representan consumo de NotDiscord.


## Referencia observada en esta instalación

El 16 de octubre de 2026 a las 23:01 (America/Lima) se midió Discord ya abierto durante 30,05 s, raíz PID 3048. Se observaron seis procesos: memoria privada media 750,500 MiB, working set medio 768,262 MiB y CPU media 0,1402 % de la capacidad total de 16 procesadores lógicos (pico 0,3684 %). No hubo contadores inaccesibles.

La actividad, el estado de llamada y la visibilidad no se inspeccionaron ni controlaron. Este resultado describe esa sesión y no demuestra ahorro frente a NotDiscord. Evidencia: evidence\benchmarks\Discord-existing-uncontrolled-state-20261017T040113096Z-pid3048.json y su CSV.
