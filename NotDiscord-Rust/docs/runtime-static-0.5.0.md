# Identidad estática del runtime preparado para 0.5.0

Inspección del 26 de octubre de 2026, sin iniciar Electron, cargar el módulo,
usar una cuenta, abrir dispositivos ni realizar pruebas funcionales. Se usó la
skill `reverse-engineer-anything` y REA para inventariar recursos PE; después,
lecturas locales de bytes acotadas a cadenas y dos exportaciones.

Raíz examinada: `.tools/temp/current/plan-050/runtime-prepared/`, relativa al
repositorio que contiene este proyecto.

| Artefacto | Bytes | SHA-256 |
| --- | ---: | --- |
| `NotDiscord.Media.exe` | 220567552 | `f92e752e394cfb28901e67321f7aa5205113643c11c0737c8e71b0c97adc671a` |
| `resources/standalone_modules/discord_voice/discord_voice.node` | 22312376 | `7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607` |

## Evidencia y alcance

- REA `inspect_pe_resources`, Evidence
  `ev_7dbc2cc42371ea57598eba53f30dd735a2f29535221646f1fa8aa5bd05b48c40`:
  host PE32+, máquina `0x8664`, 108 entradas y 51 recursos examinados. El recurso
  RT_VERSION ocupa `[219445208, 219446048)` y su SHA-256 es
  `21c1664ee3e19764bf8a0bf94a0fe1182c689e28065d9d3e86698a346370654b`.
  FileVersion `1.0.9262` es la versión del producto; no identifica Electron.
- REA `inspect_pe_resources`, Evidence
  `ev_a9d25bc874dfc88a63fbf5a24c0eda9d8df5e5954c5c9c9bcc5972e02dc61326`:
  módulo nativo PE32+, máquina `0x8664`, sin directorio de recursos. La ausencia
  de recursos no demuestra ausencia de otras formas de metadatos.
- Lectura ASCII del host: `Electron/42.11.11` en offsets de archivo `179639316`
  y `179639365`; `Electron v42.11.11` en `179551874`. Son tres declaraciones
  estáticas concordantes de **Electron 42.11.11** en el mismo artefacto,
  independientes de FileVersion. No se observó `process.versions` en ejecución.
- Lectura ASCII del host: `node_register_module_v146` en `189783748`.
  Identifica la etiqueta del ABI clásico que contiene su cargador: **146**.
  Es evidencia estática del host, no una prueba de carga del módulo suministrado.
- Lectura de la tabla PE de exportaciones del módulo: 268 nombres examinados.
  `napi_register_module_v1` se exporta en RVA `112304`, offset `109232`.
  `node_api_module_get_api_version_v1` se exporta en RVA `112288`, offset
  `109216`. Sus primeros bytes son `b8 06 00 00 00 c3`, seguidos de `cc`:
  `mov eax, 6; ret` en x64. Por tanto, el módulo declara **Node-API 6**.
  El sufijo `v1` del nombre de registro no debe confundirse con ese valor 6.

Las lecturas ASCII buscaron ocho coincidencias como máximo por cada literal de
versión/registro seleccionado. No se descompiló el host de 220 MB, ni se abrió un
desensamblador, ni se ejecutó código de los artefactos. Los offsets anteriores
son offsets del archivo completo; las RVAs sólo se usan donde se especifica.

## Desconocidos y uso en el inventario

Electron 42.11.11 y la etiqueta clásica 146 están declarados estáticamente por
el host. El módulo usa el registro Node-API y declara nivel 6; no se debe
asignarle automáticamente el ABI clásico 146 como si se hubiera verificado su
compatibilidad binaria mediante una carga real. Tampoco se estableció la versión
exacta de Node.js, la revisión del motor de voz ni el comportamiento multimedia.

El inventario de distribución puede enlazar estas declaraciones con sus hashes
y con el tipo de evidencia. Mantener campos desconocidos en `null` es preferible
a sustituirlos por FileVersion o por una supuesta equivalencia entre Node-API,
ABI clásico, arquitectura x64 y compatibilidad funcional.
