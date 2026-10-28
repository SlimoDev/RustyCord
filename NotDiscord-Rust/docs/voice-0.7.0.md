# Fuentes e intención de visualización — 0.7.0

Continuación C02 del plan del 27 de octubre. Revisión estática del código, sin
cliente, cuenta, dispositivos, llamadas ni pruebas funcionales ejecutadas.

## Causas alcanzables en 0.6.0

- El callback nativo `participant.video=false` sobrescribía `self_video` del
  Gateway. El callback `participant.present=false` retiraba incluso el roster;
  ninguno demuestra por sí solo la retirada del anuncio del emisor.
- `incoming_media_dispatch` retiraba `screen_sharing` en cualquier
  `STREAM_DELETE`, incluido el acuse del cierre de la suscripción del espectador.
  Un `CALL_UPDATE` posterior podía copiar ese falso al roster activo.
- El dibujo usaba la vida de `CallVideo` y del decoder para emitir
  `view-incoming-media visible=false`. Un snapshot intermedio podía así cancelar
  una elección autorizada. El fallback sólo incorporaba la primera fuente de
  un participante y dejaba fuera su segunda fuente.

## Contratos implementados

`CallParticipant.self_video/screen_sharing` siguen siendo anuncios del emisor.
Sólo los deltas explícitos del Gateway, la retirada de membresía y el fin de
llamada los invalidan. Un campo ausente conserva el valor anterior; un
`CALL_CREATE` nuevo no hereda anuncios de la instancia anterior. Los callbacks
del host actualizan recepción/actividad, sin retirar estos anuncios.

El actor conserva `view_intents`, máximo cuatro elecciones, una fuente por
participante, con canal y `voice_attempt`. El mapa pertenece al controlador de
la cuenta y se vacía al terminar/cambiar de llamada; no se persiste en disco.
Es independiente de `pending_views`, IDs del decoder, SSRC, visibilidad y
texturas. La proyección añade los campos compatibles `viewing` y `view_error`
con defaults al modelo serializado. La ausencia de decoder no borra la elección.

La acción explícita de dejar de ver retira sólo la intención correspondiente y
su recepción. Cambiar de fuente conserva los dos anuncios y una sola selección.
Leave, fuente explícitamente falsa y roster completo que retira al participante
limpian la intención y los recursos pertinentes. Las comprobaciones existentes
de cuenta/generación, sesión nativa y época del receptor se conservan.

`call_source_available` comparte el criterio entre UI y actor. Los bits
`camera_announcement_known/screen_announcement_known` distinguen un falso explícito
de la ausencia de anuncio en un roster provisional. Sólo la fuente desconocida
puede tener fallback desde vídeos vigentes; las dos fuentes del mismo usuario
se fusionan. Un anuncio explícito falso tiene prioridad sobre el transporte. Los previews locales conservan su
excepción propia y no se convierten en anuncios remotos.

La UI ya no ordena teardown al dibujar por retirada/reemplazo de un decoder.
Mantiene la selección y presenta espera de datos. Una nueva cámara puede heredar
la elección y reactivar su sink tras sustituir el SSRC, sin volver a unir voz.
La primera imagen y una sustitución sin imágenes tienen plazo de 35 segundos;
al vencer aparece reintento y se oculta sólo el sink de cámara, conservando la
voz y el anuncio. Minimizar pausa ese plazo y restaurar lo renueva. Pantalla
conserva los plazos y estados existentes de su suscripción independiente.

Se conserva el orden pantalla arriba/cámara abajo y se corrige la expectativa
antigua contraria en la fuente de un test existente. También se actualiza su
expectativa de `STREAM_DELETE`: la retirada real se demuestra ahora con
`VOICE_STATE_UPDATE self_stream=false`. Ninguno se ejecutó en esta tarea.

## Pendiente de confirmación

La revisión estática no identifica la secuencia exacta que ocurrió en la
instalación del usuario ni acredita recepción real. Queda confirmar navegación
DM/grupo/guild/Home, retiradas y cambios de SSRC con ambas fuentes, minimizar y
restaurar, reintentos, DPI y ventanas pequeñas. No se cambió el host multimedia,
el cifrado, los controles de volumen ni la conexión de audio para simular éxito.
