//! A silent synthetic toast/history/remove probe for the EXISTING app identity.
//! Does not register an app, write shortcuts, start Gateway, inspect notification
//! contents belonging to others, or remove any toast except its own UUID pair.
//! --image adds an owned local PNG and checks only its own retained toast XML.
//! https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.suppresspopup
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::{
    core::HSTRING,
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationHistory, ToastNotificationManager,
    },
};
const APP_ID: &str = "com.notdiscord.Terminal.Native";
struct OwnImage {
    root: PathBuf,
    path: PathBuf,
    uri: String,
}
impl OwnImage {
    fn create() -> Result<Self, String> {
        let root = std::env::temp_dir().join(format!(
            "notdiscord-synthetic-toast-{}",
            uuid::Uuid::new_v4()
        ));
        let directory = root.join("notification-images");
        std::fs::create_dir_all(&directory)
            .map_err(|_| "Cannot create owned image fixture directory")?;
        let name = format!("{:x}.png", Sha256::digest(uuid::Uuid::new_v4().as_bytes()));
        let mut image = Self {
            path: directory.join(name),
            root,
            uri: String::new(),
        };
        image::RgbaImage::from_fn(96, 96, |x, y| {
            image::Rgba([
                if (x / 12 + y / 12) % 2 == 0 { 180 } else { 60 },
                80,
                220,
                255,
            ])
        })
        .save_with_format(&image.path, image::ImageFormat::Png)
        .map_err(|_| "Cannot create synthetic local PNG")?;
        image.uri = notdiscord::notification_identity::toast_image_uri(
            image.path.to_str().ok_or("Fixture path is not UTF-8")?,
        )
        .ok_or("Production URI validation rejected the owned PNG")?;
        Ok(image)
    }
}
impl Drop for OwnImage {
    fn drop(&mut self) {
        // Exact files and empty owned directories only; never recursive deletion.
        if self.path.parent() == Some(self.root.join("notification-images").as_path()) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(self.root.join("notification-images"));
            let _ = std::fs::remove_dir(&self.root);
        }
    }
}
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
struct OwnToast {
    history: ToastNotificationHistory,
    tag: HSTRING,
    group: HSTRING,
    app: HSTRING,
    armed: bool,
}
impl OwnToast {
    fn own_image_retained(&self, uri: &str) -> Result<bool, String> {
        let entries = self
            .history
            .GetHistoryWithId(&self.app)
            .map_err(|_| "Cannot inspect own image toast identifiers")?;
        let size = entries
            .Size()
            .map_err(|_| "Cannot inspect own image toast identifiers")?;
        if size > 4096 {
            return Err("Unexpectedly large app notification history".into());
        }
        for index in 0..size {
            let entry = entries
                .GetAt(index)
                .map_err(|_| "Cannot inspect own image toast identifiers")?;
            if entry.Tag().ok().as_ref() == Some(&self.tag)
                && entry.Group().ok().as_ref() == Some(&self.group)
            {
                // Read content ONLY after proving ownership by both fresh UUIDs.
                let xml = entry
                    .Content()
                    .and_then(|xml| xml.GetXml())
                    .map_err(|_| "Cannot inspect this probe's own retained XML")?
                    .to_string();
                return Ok(xml.contains("appLogoOverride") && xml.contains(&xml_escape(uri)));
            }
        }
        Ok(false)
    }
    fn present(&self) -> Result<bool, String> {
        let entries = self
            .history
            .GetHistoryWithId(&self.app)
            .map_err(|_| "Windows did not expose this app's notification history")?;
        let size = entries
            .Size()
            .map_err(|_| "Cannot inspect history identifiers")?;
        if size > 4096 {
            return Err("Unexpectedly large app notification history".into());
        }
        for index in 0..size {
            let entry = entries
                .GetAt(index)
                .map_err(|_| "Cannot inspect history identifiers")?;
            // The history API has no tag lookup. Compare identifiers only;
            // never call Content(), Launch(), XML access, or log other entries.
            if entry.Tag().ok().as_ref() == Some(&self.tag)
                && entry.Group().ok().as_ref() == Some(&self.group)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn wait_for(&self, wanted: bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.present()? == wanted {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(if wanted {
                    "The silent synthetic toast was not recorded in app history"
                } else {
                    "Windows did not confirm removal of the synthetic toast"
                }
                .into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn remove(&self) -> Result<(), String> {
        self.history
            .RemoveGroupedTagWithId(&self.tag, &self.group, &self.app)
            .map_err(|_| {
                "Windows could not remove the synthetic toast by its exact UUID identifiers".into()
            })
    }
}
impl Drop for OwnToast {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.remove();
        }
    }
}
fn probe() -> Result<(), String> {
    let with_image = std::env::args()
        .skip(1)
        .any(|argument| argument == "--image");
    let image = with_image.then(OwnImage::create).transpose()?;
    // Use the same windows-core activation fallback as the production UI.
    // Process identity only: deliberately NEVER call notification_identity::register.
    notdiscord::notification_identity::set_app_id();
    let app = HSTRING::from(APP_ID);
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app)
        .map_err(|_| "The existing app identity is not available to Windows notifications")?;
    if notifier
        .Setting()
        .map_err(|_| "Windows notification settings are unavailable")?
        != NotificationSetting::Enabled
    {
        return Err(
            "Windows notifications are disabled for this app; settings were not changed".into(),
        );
    }
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let mut own = OwnToast {
        history: ToastNotificationManager::History()
            .map_err(|_| "Windows notification history is unavailable")?,
        tag: HSTRING::from(&uuid[..16]),
        group: HSTRING::from(&uuid[16..]),
        app,
        armed: false,
    };
    if own.present()? {
        return Err("UUID identifiers already exist; refusing to overwrite a toast".into());
    }
    let xml = XmlDocument::new().map_err(|_| "Cannot create synthetic notification XML")?;
    let source = image
        .as_ref()
        .map(|image| {
            format!(
                "<image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"{}\"/>",
                xml_escape(&image.uri)
            )
        })
        .unwrap_or_default();
    let payload=format!("<toast><visual><binding template=\"ToastGeneric\"><text>NotDiscord · comprobación técnica</text><text>Aviso sintético temporal, sin mensajes de una cuenta.</text>{source}</binding></visual><audio silent=\"true\"/></toast>");
    xml.LoadXml(&HSTRING::from(payload))
        .map_err(|_| "Cannot prepare synthetic notification XML")?;
    let toast = ToastNotification::CreateToastNotification(&xml)
        .map_err(|_| "Cannot prepare synthetic toast")?;
    toast.SetTag(&own.tag).map_err(|_| "Cannot set UUID tag")?;
    toast
        .SetGroup(&own.group)
        .map_err(|_| "Cannot set UUID group")?;
    // Failure to suppress UI aborts BEFORE Show: there is no popup fallback.
    toast
        .SetSuppressPopup(true)
        .map_err(|_| "This Windows version cannot suppress the popup; probe aborted")?;
    if !toast
        .SuppressPopup()
        .map_err(|_| "Cannot confirm suppression")?
    {
        return Err("Popup suppression was not confirmed; probe aborted".into());
    }
    own.armed = true;
    notifier
        .Show(&toast)
        .map_err(|_| "Windows rejected the silent synthetic toast")?;
    own.wait_for(true)?;
    let image_retained = if let Some(image) = &image {
        if !own.own_image_retained(&image.uri)? {
            return Err(
                "Windows did not retain the local image source in this probe's own toast".into(),
            );
        }
        true
    } else {
        false
    };
    own.remove()?;
    own.wait_for(false)?;
    println!(
        "{}",
        serde_json::json!({"syntheticOnly":true,"popupSuppressed":true,"historyConfirmed":true,"exactUuidRemoved":true,"appRegistrationChanged":false,"gatewayStarted":false,"otherNotificationContentsRead":false,"localImageRequested":with_image,"localImageXmlRetained":image_retained,"imagePixelsVisuallyVerified":false})
    );
    Ok(())
}
fn main() {
    if let Err(error) = probe() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
