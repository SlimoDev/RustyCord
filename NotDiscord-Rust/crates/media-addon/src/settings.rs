use crate::protocol::{MediaError, Result, string, volume};
use serde_json::{Value, json};
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub struct AudioSettings {
    pub input: String,
    pub output: String,
    pub input_volume: f64,
    pub sensitivity_db: f64,
    pub output_volume: f64,
    pub stream_volume_percent: f64,
    pub stream_volume_revision: u64,
    pub noise: bool,
    pub echo: bool,
    pub ptt: bool,
    pub user_volumes: HashMap<String, f64>,
    pub user_volumes_revision: u64,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input: "default".into(),
            output: "default".into(),
            input_volume: 100.,
            sensitivity_db: -40.,
            output_volume: 100.,
            stream_volume_percent: 100.,
            stream_volume_revision: 0,
            noise: true,
            echo: true,
            ptt: false,
            user_volumes: HashMap::new(),
            user_volumes_revision: 0,
        }
    }
}
impl AudioSettings {
    pub fn parse(p: &Value) -> Result<Self> {
        let mut s = Self::default();
        for (key, target) in [("inputId", &mut s.input), ("outputId", &mut s.output)] {
            if let Some(v) = p.get(key) {
                *target = string(v, key, 512)?;
            }
        }
        for (key, target) in [
            ("inputVolume", &mut s.input_volume),
            ("outputVolume", &mut s.output_volume),
        ] {
            if let Some(v) = p.get(key) {
                *target = volume(v)? * 100.;
            }
        }
        if s.input_volume > 200. {
            return Err(MediaError::new(
                "INVALID_VOLUME",
                "Input volume must not exceed 200%.",
            ));
        }
        if let Some(value)=p.get("streamVolumePercent") {
            s.stream_volume_percent=value.as_f64().filter(|v|v.is_finite()&&(0.0..=100.0).contains(v))
                .ok_or_else(||MediaError::new("INVALID_STREAM_VOLUME","Stream volume must be between 0 and 100%."))?;
        }
        if let Some(value)=p.get("streamVolumeRevision") {
            s.stream_volume_revision=value.as_u64().ok_or_else(||MediaError::new("INVALID_VOLUME_REVISION","Invalid stream volume revision."))?;
        }
        if let Some(value) = p.get("inputSensitivityDb") {
            s.sensitivity_db = value
                .as_f64()
                .filter(|v| v.is_finite() && (-100.0..=0.0).contains(v))
                .ok_or_else(|| {
                    MediaError::new(
                        "INVALID_SENSITIVITY",
                        "Input sensitivity must be between -100 and 0 dB.",
                    )
                })?;
        }
        for (key, target) in [
            ("noiseSuppression", &mut s.noise),
            ("echoCancellation", &mut s.echo),
            ("pushToTalk", &mut s.ptt),
        ] {
            if let Some(v) = p.get(key) {
                *target = v.as_bool().ok_or_else(|| {
                    MediaError::new("INVALID_AUDIO_SETTING", "Audio switches must be booleans.")
                })?;
            }
        }
        if let Some(value)=p.get("userVolumes") {s.user_volumes=crate::protocol::user_volumes(value)?;}
        if let Some(value)=p.get("userVolumesRevision") {
            s.user_volumes_revision=value.as_u64().ok_or_else(||MediaError::new("INVALID_VOLUME_REVISION","Invalid participant volume revision."))?;
        }
        Ok(s)
    }
    pub fn json(&self) -> Value {
        json!({"inputId":self.input,"outputId":self.output,"inputVolume":self.input_volume,"outputVolume":self.output_volume,"streamVolumePercent":self.stream_volume_percent,"streamVolumeRevision":self.stream_volume_revision,"inputSensitivityDb":self.sensitivity_db,"noiseSuppression":self.noise,"echoCancellation":self.echo,"pushToTalk":self.ptt})
    }
    pub fn processing(&self) -> Value {
        json!({"builtInEchoCancellation":true,"echoCancellation":self.echo,"noiseSuppression":self.noise,"automaticGainControl":true,"noiseCancellation":false})
    }
    pub fn mode(&self) -> Value {
        // Both manual VAD and a physical PTT key use one native transmit gate.
        // The Rust host chooses its input from captured levels or key state;
        // a second automatic native classifier cannot override that decision.
        json!({"inputMode":2,"inputModeOptions":{"pttReleaseDelay":0}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation() {
        assert_eq!(AudioSettings::parse(&json!({})).unwrap().input, "default");
        assert!(AudioSettings::parse(&json!({"inputVolume":201})).is_err());
        assert!(AudioSettings::parse(&json!({"pushToTalk":1})).is_err());
        assert_eq!(
            AudioSettings::parse(&json!({"pushToTalk":true}))
                .unwrap()
                .mode()["inputMode"],
            2
        );
    }
    #[test]
    fn sensitivity_is_validated_and_native_transmission_uses_one_explicit_gate() {
        assert_eq!(
            AudioSettings::parse(&json!({})).unwrap().sensitivity_db,
            -40.
        );
        for threshold in [-100., -42.5, 0.] {
            let settings = AudioSettings::parse(&json!({"inputSensitivityDb":threshold})).unwrap();
            assert_eq!(settings.mode()["inputMode"],2);
            assert_eq!(settings.mode()["inputModeOptions"]["pttReleaseDelay"],0);
            assert_eq!(settings.json()["inputSensitivityDb"], threshold);
        }
        for value in [json!(-101), json!(1), json!("-40"), Value::Null] {
            assert!(AudioSettings::parse(&json!({"inputSensitivityDb":value})).is_err());
        }
        let settings =
            AudioSettings::parse(&json!({"inputSensitivityDb":-30,"pushToTalk":true})).unwrap();
        assert_eq!(
            settings.mode(),
            json!({"inputMode":2,"inputModeOptions":{"pttReleaseDelay":0}})
        );
    }
}
