# ABI estática de streamParameters — continuación del 4/11/2026

Artefacto explícito: `media-host/runtime/resources/standalone_modules/discord_voice/discord_voice.node`, SHA-256 `7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607`. PE32+ x64, base preferida `0x180000000`. No se cargó la DLL, no se ejecutó el motor y no se accedió a cuentas o dispositivos.

## Resultado

La hipótesis de que un delta `streamParameters` que sólo contiene vídeo retira implícitamente el descriptor de audio **no corresponde a la ruta nativa examinada de este artefacto**. La aplicación trata las entradas por tipo y, para vídeo, por RID; modifica las entradas correspondientes. No sustituye por esa mera omisión toda la colección audio/vídeo ni pone a cero el objeto de audio existente.

Es una deducción estática del flujo de instrucciones, no una observación de transmisión. No acredita continuidad de callbacks, puerta de captura, DAVE, paquetes ni audibilidad; no identifica todavía la causa del incidente real. Otras propiedades de `setTransportOptions` sí pueden modificar códec/bitrate/controles de audio cuando se suministran expresamente.

## Herramientas y límites

`REA binary_session` se consultó una vez para verificar disponibilidad actual. Sigue sin binding nativo: Ghidra no configurado (`GHIDRA_INSTALL_DIR` ausente), IDA sin configuración y Hopper no admitido en Windows. No se repitió la decompilación previamente rechazada y no se instaló ni configuró un proveedor.

Se encontró Microsoft `dumpbin.exe` 14.44.35207.1 en el toolchain MSVC existente y Python 3.13 existente. Se usaron `/headers`, `/exports` y `/disasm:nobytes /range:...`, más un índice PE con la biblioteca estándar de Python. **Estos resultados son análisis ordinario por shell, no Evidence producido por un proveedor REA.** Las instrucciones desensambladas se preservan en los archivos `t2-abi-*.asm`; los rangos proceden de `.pdata` cuando están disponibles y no se presentan como decompilación de fuentes originales.

`t2-abi-scan.py` y `t2-abi-string-index.json` reproducen el inventario acotado: literales ASCII terminados en NUL, offsets de archivo, VA preferidas y candidatos de LEA relativa a RIP en `.text`. La confirmación de instrucciones se hizo después con dumpbin. El índice no pretende recuperar referencias indirectas ni un grafo completo de funciones.

## Cadena acreditada por bytes e instrucciones

| Paso | Ubicación en el PE | Evidencia |
|---|---|---|
| Nombre JS y callback | literal `setTransportOptions` en VA `0x180eda607`; registro `0x18001a3b0`–`0x18001a3d0` | El registro asigna el callback `0x18003a470`. |
| Destino de la captura C++ | `0x18001954e`–`0x180019563` | La captura del setter apunta a `0x180024150`. El texto de diagnóstico en `0x180f0f536` identifica `VoiceConnectionWrapper::SetTransportOptions(n::Object)`. |
| Parseo de propiedad | `0x180026eba`–`0x180026ed4`, `0x180027429`–`0x180027493` | Busca el literal `streamParameters`, invoca el parser del vector `0x18003f560` y guarda vector más flag de presencia. Campo ausente no se trata como una lista presente vacía. |
| Tipo de cada entrada | `0x18003fd91`–`0x18003fdca` | Compara los bytes de `audio` y `video`: enum `0` para audio, `1` para vídeo. |
| Construcción de ajustes | `0x1800275dc`–`0x180027640` | Mueve los parámetros y flags a la estructura de ajustes de la conexión. |
| Aplicación a esa conexión | `0x180027687`–`0x1800276be` | Llama al export `?ApplySettings@Connection@voice@discord@@...`, RVA `0x196960`. |
| Tarea de la conexión | `0x1800196960` y task `0x180019f570`–`0x180019f57f` | Copia los ajustes y los entrega a la implementación de la conexión, `0x1800fff00`. |
| Entrega de transporte | `0x18010006e`–`0x180100200` | Comprueba el flag del subobjeto; copia vector/flag y llama a `0x18014a4f0`. |
| Entrada de la lista | `0x18014a6cb`–`0x18014a78e` | Requiere flag de presencia `+0x180`, copia el vector `+0x168` e itera descriptores de `0x50` bytes. |
| Audio | `0x18014a8a0`–`0x18014a9e2`, `0x18014a9e2`–`0x18014ab27` | La entrada tipo `0` modifica el descriptor de audio existente, obtenido de `this+0x1d98`, si se aportan SSRC/bitrate. |
| Vídeo | `0x18014a93c`–`0x18014a9dd`, `0x18014ab48`–`0x18014acfa` | La entrada tipo `1` busca RID en el vector `this+0x22b8..0x22c0`; encuentra/actualiza o añade el descriptor de vídeo. |
| Final de la lista | `0x18014b017`–`0x18014b05e`, `0x18014b3c6`–`0x18014b4d4` | Se destruye la copia temporal y se aplican sólo las banderas de cambios relevantes; la mera ausencia de entrada audio no elimina su descriptor. |

Las direcciones son VA preferidas del archivo (no ASLR de un proceso real). La correlación de los objetos de audio/vídeo se apoya en la rama enum, accesos persistentes y el uso posterior de esas estructuras; no depende únicamente del nombre de un string.

## Propiedad global que aún requiere observación

El wrapper `index.js:203–208` crea `VoiceConnection` y establece `createOwnStreamConnectionWithOptions` como alias de esa misma fábrica. No hay en ese alias una clase nativa diferente ni una garantía de aislamiento del engine global. El tipo de conexión sigue configurándose mediante sus opciones.

El PE exporta `Connection::SetGoLiveDevices` y `Connection::ClearGoLiveDevices`, y el wrapper los expone. La búsqueda acotada en `Host`, `native.rs`, `video.rs` y `settings.rs` no encontró callers de esos dos métodos en la implementación actual. No se propone invocarlos para arreglar una ruta que no los usa. El PE también exporta `GetPTTActivationState`, pero `bindConnectionInstance` no publica un getter equivalente; no se lo considera un punto de diagnóstico JS disponible por ese mero export.

Para el ensayo sintético nativo preparado por el implementador, los puntos pertinentes siguen siendo creación/cierre de la conexión OwnStream, callback/eligibilidad de entrada y estadísticas saneadas de la conexión principal. Un incremento de contadores sólo bajo fakes no acredita codificación o envío real. La hipótesis de pérdida del micrófono por reconfiguración global o por una transición DAVE no queda descartada por este análisis de la lista.

## Consumo nativo de los flags de captura ficticia

En el mismo artefacto se comprobó que `useFakeAudioCapture: true` y `useFakeVideoCapture: true` son campos consumidos por la inicialización nativa, con ramas de selección de fuentes generadas. Esta conclusión tiene soporte en parser, copias de estructura y consumidor; no se infiere sólo del caller `index.js:359–370`.

| Paso | Direcciones preferidas | Evidencia |
|---|---|---|
| Registro de initialize | `0x1800146a5`–`0x1800146d6` | Captura la función `0x1800053b0` y la registra con el literal `initialize` en `0x180eea140`. |
| Lectura de flags | `0x1800058b2`–`0x1800058c6`, `0x180005911`–`0x180005925` | Literales `useFakeVideoCapture` en `0x180eec3dd` y `useFakeAudioCapture` en `0x180eec3ae`. Llama al lector booleano `0x180005dd0` y guarda los resultados en `EngineConfig+0x50` y `+0x78`. La estructura local comienza en `rsp+0x150`. |
| Copia de configuración | `0x180004aeb`–`0x180004aef`, `0x180004b9c`–`0x180004ba0` | El copiador `0x180004990` conserva ambos bytes. El parser lo llama en `0x180005ab4`, y entrega la copia al constructor en `0x180005abf`. |
| Tarea inicializadora | `0x1800960f3`–`0x18009613a`, `0x18009645c`–`0x180096495` | El constructor `0x180095e40` copia los flags al objeto de tarea. Su tabla `0x180dbb0f0` tiene ejecutor `0x1800bb540`, que salta a `0x1800bb140`. Éste copia la configuración mediante `0x1800bb240` y llama al export `MediaEngine::MediaEngine(...,EngineConfig const&)` en `0x1800bb1b1`. |
| Motor y conservación | `0x180180246`, `0x18018035d`, `0x180118255`–`0x18011825f` | La construcción lleva el mismo `EngineConfig` a `0x180117e10`; allí queda conservado en `Engine+0x548`. El flag audio está por ello en `Engine+0x5c0`. |
| Audio: adaptación | `0x180119c39`–`0x180119c40`, `0x180119ccc`, `0x18011ac82`–`0x18011acf1`, `0x1800eb1fc`–`0x1800eb255` | Copia `Engine+0x5c0` al byte `+5` de la configuración del ADM; sucesivas copias mantienen ese byte y llegan a `0x1800ebac0`. |
| Audio: factory real | `0x1800ebb09`–`0x1800ebb1e`, `0x1800d5c80` | Llama al segundo slot de la factory conservada por el motor. La factory por defecto se construye con tabla `0x180dc0640`, cuyo slot `+8` apunta a `0x1800d5c80`. |
| Audio: controles offload | `0x1800d5f0c`, `0x1800d6003`–`0x1800d6027`, `0x1800d6b26`–`0x1800d6b34` | Sin offload llama a `0x1800d6440` directamente. Con offload crea tarea con tabla `0x180dbc260`; su ejecutor `0x1800d6ae0` llama a la misma `0x1800d6440` y conserva la configuración. |
| Audio: elección fake | `0x1800d6468`–`0x1800d64d1` | Compara el byte `ADMConfig+5` con `1`: verdadero toma la rama `0x1800d648b` y llama a `0x1800de980`. El camino alternativo sin flag ni ficheros empieza en `0x1800d6588`; no se alcanza por esa condición verdadera. |
| Audio: productor local | `0x1800de9b6`–`0x1800de9c0`, `0x1800dea99`, `0x1800e38a0`, `0x1800e38e0` | La factory vinculada al literal `../../discord_native_lib/src/media/audio_device/fake_adm.cpp` selecciona el productor local `0x1800e38a0` cuando el flag es verdadero. Su tabla `0x180dbcee0` apunta al generador `0x1800e38e0`, que calcula y escribe muestras; no es una selección de GUID físico. |
| Vídeo: adaptación | `0x180119894`–`0x180119897`, `0x18011aa34`–`0x18011aaa8` | Pasa el `EngineConfig` a `0x18011a9f0`, copia el byte `+0x50` a la configuración de vídeo e invoca `0x1801eecf0`. |
| Vídeo: elección fake | `0x1801eed09`–`0x1801eed67` | True crea una fuente mediante `0x1801e7460`, etiquetada `Strobe Video Source` (literal `0x180dc2110`), y salta al retorno común. La rama alternativa sin flag ni fichero llama a `0x1801fc320` en `0x1801eee42`. |

La selección de backend ficticio está acreditada para estos dos campos y este hash. No es una auditoría completa de todas las interacciones que puede tener la inicialización del motor con Windows ni una prueba de ejecución. Para el probe se mantienen además entrada y salida deshabilitadas, salida a volumen cero, transporte sólo `127.0.0.1`, y activación de entrada únicamente ante una identidad sintética explícita del catálogo; no se deduce que `Default` sea seguro por el flag. Si no existe esa identidad explícita, se conserva la entrada cerrada y la continuidad de captura queda sin verificar. No se ejecutó el probe durante este análisis.

Los nuevos desensamblados `t2-abi-initialize-*.asm`, `t2-abi-config-copy.asm`, `t2-abi-engine-*.asm`, `t2-abi-media*.asm`, `t2-abi-audio-*.asm`, `t2-abi-adm-*.asm`, `t2-abi-fake*.asm` y `t2-abi-video-*.asm` conservan la cadena. El índice PE reproduce las lecturas de tablas y literales, con guarda del SHA-256 para sus direcciones específicas.
