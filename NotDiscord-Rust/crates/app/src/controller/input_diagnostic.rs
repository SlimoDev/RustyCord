use super::*;
impl Controller {
    pub(super) fn input_diagnostic_action(&mut self,action:&UiAction)->Result<bool,String> {
        if !matches!(action.kind.as_str(),"start-input-diagnostic"|"cancel-input-diagnostic"){return Ok(false);}
        let scope:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        if scope["accountId"].as_str()!=Some(self.me().as_str())
            ||scope["generation"].as_u64()!=Some(self.state.notification_generation)
            ||scope["voiceAttempt"].as_u64()!=Some(self.voice_attempt){return Ok(true);}
        let media=self.media.clone().ok_or("Entra a una llamada para observar el micrófono.")?;
        let bridge_scope=media.diagnostic_scope();
        if action.kind=="cancel-input-diagnostic" {
            if let Some((scope,capture))=self.voice_lifecycle.input_capture.filter(|(scope,_)|*scope==bridge_scope){
                let mut done=UiAction::id("__voice-data","input-diagnostic");done.extra=self.meta().to_string();done.text=capture.to_string();
                self.launch(done,None,async move {Ok(wrapped(media.command("cancelInputDiagnostic",json!({"capture":capture})).await))});
                // Revoking observation never declares a native query finished.
                crate::diagnostics::end_voice_capture(scope,capture);
            }
            return Ok(true);
        }
        if !self.state.voice_connected||self.voice_lifecycle.native_voice.is_none(){return Err("La voz debe estar conectada antes de iniciar la observación.".into());}
        self.voice_lifecycle.input_capture_sequence=self.voice_lifecycle.input_capture_sequence.wrapping_add(1).max(1);
        let capture=self.voice_lifecycle.input_capture_sequence;
        if !crate::diagnostics::begin_voice_capture(bridge_scope,capture){return Err("Ya hay una observación de micrófono en curso.".into());}
        self.voice_lifecycle.input_capture=Some((bridge_scope,capture));
        let mut done=UiAction::id("__voice-data","input-diagnostic");done.extra=self.meta().to_string();done.text=capture.to_string();
        self.launch(done,None,async move {Ok(wrapped(media.command("startInputDiagnostic",json!({"capture":capture})).await))});
        self.status("Observando hasta diez segundos de contadores de micrófono, sin grabar audio.");Ok(true)
    }
    pub(super) fn complete_input_diagnostic(&mut self,action:&UiAction,value:&Value) {
        if unwrapped(value).is_err()&&self.voice_lifecycle.input_capture.is_some_and(|(_,capture)|Some(capture)==action.text.parse().ok()){
            if let Some((scope,capture))=self.voice_lifecycle.input_capture.take(){crate::diagnostics::end_voice_capture(scope,capture);}
            self.status("No se confirmó la observación del micrófono. No se repetirá automáticamente.");
        }
    }
}
