//! Read-only viewer shared by prepared and received text attachments. Bytes are
//! read and validated off the UI thread; content is shown, never interpreted.
use super::*;
use std::{io::Read, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};

const INPUT_LIMIT: usize = 1024 * 1024;
const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
// Long lines become fixed rows, so one huge line never lays out as one galley.
const ROW_CHARS: usize = 200;
const INVALID: &str = "El archivo no es texto válido en UTF-8 o UTF-16 con marca de orden.";
const TOO_LARGE: &str = "El archivo supera 1 MiB y no se puede previsualizar. Sus límites de envío no cambian.";

enum Source {
    Local(PathBuf),
    Remote { account: String, generation: u64, channel: String, message: String, attachment: String, url: String },
}
#[derive(Clone)]
struct Document { text: Arc<str>, rows: Arc<Vec<(u32, u32)>> }
enum Outcome { Ready(Document), Expired, Failed(String) }
struct Load { cancel: Arc<AtomicBool>, slot: Arc<Mutex<Option<Outcome>>> }
impl Drop for Load {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Release); }
}
enum View { Loading, Ready(Document), Expired, Failed(String) }
pub(super) struct TextViewer {
    operation: u64,
    name: String,
    source: Source,
    load: Option<Load>,
    view: View,
    refreshed: bool,
}
pub(super) fn text_attachment_name(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
}
pub(super) fn text_attachment(attachment: &ChatAttachment) -> bool {
    attachment.content_type.as_deref().is_some_and(|kind| kind.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/plain"))
        || text_attachment_name(&attachment.name)
}
fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, &'static str> {
    if bytes.len() % 2 != 0 { return Err(INVALID); }
    char::decode_utf16(bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]])))
        .collect::<Result<String, _>>().map_err(|_| INVALID)
}
/// Strict by design: a BOM selects UTF-16, and everything else must be UTF-8.
/// Binary data is rejected instead of guessing a legacy codepage or truncating.
fn decode(bytes: &[u8]) -> Result<String, &'static str> {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        std::str::from_utf8(rest).map_err(|_| INVALID)?.to_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        decode_utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        decode_utf16(rest, u16::from_be_bytes)?
    } else {
        std::str::from_utf8(bytes).map_err(|_| INVALID)?.to_owned()
    };
    if text.len() > OUTPUT_LIMIT { return Err(TOO_LARGE); }
    if text.contains('\0') { return Err(INVALID); }
    Ok(text)
}
fn layout_rows(text: &str) -> Vec<(u32, u32)> {
    let mut rows = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let mut from = start;
        for (count, (index, _)) in body.char_indices().enumerate() {
            if count > 0 && count % ROW_CHARS == 0 {
                rows.push((from as u32, (start + index) as u32));
                from = start + index;
            }
        }
        rows.push((from as u32, (start + body.len()) as u32));
        start += line.len();
    }
    if rows.is_empty() { rows.push((0, 0)); }
    rows
}
fn document(bytes: &[u8]) -> Outcome {
    match decode(bytes) {
        Ok(text) => {
            let rows = Arc::new(layout_rows(&text));
            Outcome::Ready(Document { text: text.into(), rows })
        }
        Err(error) => Outcome::Failed(error.into()),
    }
}
/// Reads limit+1 bytes, so a file that grows after opening is still detected.
fn bounded(mut reader: impl Read, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    while bytes.len() <= INPUT_LIMIT {
        if cancel.load(Ordering::Acquire) { return Err("Lectura cancelada.".into()); }
        let remaining = (INPUT_LIMIT + 1 - bytes.len()).min(chunk.len());
        let count = reader.read(&mut chunk[..remaining]).map_err(|_| "No se pudo leer el archivo.".to_owned())?;
        if count == 0 { return Ok(bytes); }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Err(TOO_LARGE.into())
}
fn read_local(path: &std::path::Path, cancel: &AtomicBool) -> Outcome {
    let result = std::fs::File::open(path).map_err(|_| "No se pudo abrir el adjunto.".to_owned()).and_then(|file| {
        let metadata = file.metadata().map_err(|_| "No se pudo comprobar el adjunto.".to_owned())?;
        if !metadata.is_file() { return Err("El adjunto no es un archivo.".into()); }
        if metadata.len() > INPUT_LIMIT as u64 { return Err(TOO_LARGE.to_owned()); }
        bounded(file, cancel)
    });
    result.map_or_else(Outcome::Failed, |bytes| document(&bytes))
}
fn allowed_source(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| url.scheme() == "https" && url.port().is_none()
        && url.username().is_empty() && url.password().is_none()
        && matches!(url.host_str(), Some("cdn.discordapp.com" | "media.discordapp.net")))
}
/// Uses the media client: no credentials, no cookies and no automatic redirect.
fn read_remote(url: &str, cancel: &AtomicBool) -> Outcome {
    if !allowed_source(url) { return Outcome::Failed("El adjunto no procede de un origen permitido.".into()); }
    let Some(client) = resources::client() else { return Outcome::Failed("La descarga no está disponible.".into()); };
    let Ok(response) = client.get(url).send() else {
        return Outcome::Failed("No se pudo descargar el adjunto. Comprueba la conexión.".into());
    };
    match response.status().as_u16() {
        200 => {}
        401 | 403 | 404 | 410 => return Outcome::Expired,
        429 => return Outcome::Failed("El servidor limitó las descargas. Inténtalo de nuevo en unos segundos.".into()),
        _ => return Outcome::Failed("El servidor no entregó el adjunto.".into()),
    }
    if response.content_length().is_some_and(|length| length > INPUT_LIMIT as u64) {
        return Outcome::Failed(TOO_LARGE.into());
    }
    bounded(response, cancel).map_or_else(Outcome::Failed, |bytes| document(&bytes))
}
impl TextViewer {
    fn new(name: &str, source: Source) -> Self {
        Self { operation: rand::random(), name: name.chars().take(200).collect(), source,
            load: None, view: View::Loading, refreshed: false }
    }
    fn start(&mut self, ctx: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(None));
        let (worker_cancel, worker_slot, worker_ctx) = (cancel.clone(), slot.clone(), ctx.clone());
        let source = match &self.source {
            Source::Local(path) => Ok(path.clone()),
            Source::Remote { url, .. } => Err(url.clone()),
        };
        let started = std::thread::Builder::new().name("text-viewer".into()).spawn(move || {
            let outcome = match &source {
                Ok(path) => read_local(path, &worker_cancel),
                Err(url) => read_remote(url, &worker_cancel),
            };
            if !worker_cancel.load(Ordering::Acquire) {
                *worker_slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(outcome);
                worker_ctx.request_repaint();
            }
        });
        self.view = if started.is_ok() { View::Loading } else { View::Failed("No se pudo iniciar la lectura.".into()) };
        self.load = started.is_ok().then_some(Load { cancel, slot });
    }
    fn poll(&mut self) {
        let Some(load) = &self.load else { return; };
        let Some(outcome) = load.slot.lock().unwrap_or_else(|error| error.into_inner()).take() else { return; };
        self.load = None;
        self.view = match outcome {
            Outcome::Ready(document) => View::Ready(document),
            Outcome::Expired if self.refreshed => View::Failed("El servidor ya no entrega este adjunto.".into()),
            Outcome::Expired => View::Expired,
            Outcome::Failed(error) => View::Failed(error),
        };
    }
    fn revoked(&self, state: &AppSnapshot) -> bool {
        let Source::Remote { account, generation, channel, .. } = &self.source else { return false; };
        state.current_user.as_ref().is_none_or(|user| user.id != *account)
            || state.notification_generation != *generation
            || chat::read_revoked(state, channel)
    }
}
impl NotDiscordUi {
    pub(super) fn open_local_text(&mut self, path: &std::path::Path, name: &str) {
        let mut viewer = TextViewer::new(name, Source::Local(path.to_owned()));
        viewer.start(&self.ctx);
        self.close_text_viewer();
        self.text_viewer = Some(viewer);
    }
    pub(super) fn open_remote_text(&mut self, message: &ChatMessage, attachment: &ChatAttachment, state: &AppSnapshot) {
        let Some(account) = state.current_user.as_ref().map(|user| user.id.clone()) else { return; };
        let mut viewer = TextViewer::new(&attachment.name, Source::Remote {
            account, generation: state.notification_generation, channel: message.channel_id.clone(),
            message: message.id.clone(), attachment: attachment.id.clone(), url: attachment.url.clone(),
        });
        viewer.start(&self.ctx);
        self.close_text_viewer();
        self.text_viewer = Some(viewer);
    }
    pub(super) fn text_viewer_holds(&self, path: &PathBuf) -> bool {
        self.text_viewer.as_ref().is_some_and(|viewer| matches!(&viewer.source, Source::Local(held) if held == path))
    }
    /// Ends the load and releases the viewer's reference to a prepared file.
    pub(super) fn close_text_viewer(&mut self) {
        let Some(viewer) = self.text_viewer.take() else { return; };
        if let Source::Local(path) = &viewer.source {
            let path = path.clone();
            drop(viewer);
            self.cleanup_unreferenced_temporary(&[path]);
        }
    }
    pub(super) fn receive_attachment_url(&mut self, data: &Value) {
        let ctx = self.ctx.clone();
        let Some(viewer) = self.text_viewer.as_mut() else { return; };
        if data["operation"].as_u64() != Some(viewer.operation) { return; }
        let error = s(data, "error");
        if !error.is_empty() { viewer.view = View::Failed(error.into()); return; }
        if let Source::Remote { url, .. } = &mut viewer.source { *url = s(data, "url").to_owned(); }
        viewer.refreshed = true;
        viewer.start(&ctx);
    }
    pub(super) fn draw_text_viewer(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        if self.text_viewer.as_ref().is_some_and(|viewer| viewer.revoked(state)) { self.close_text_viewer(); }
        let Some(viewer) = self.text_viewer.as_mut() else { return; };
        viewer.poll();
        let maximum = (ctx.content_rect().size() - egui::vec2(32., 64.)).max(egui::vec2(280., 180.));
        let mut open = true;
        let mut refresh = None;
        egui::Window::new(RichText::new(&viewer.name)).id(egui::Id::new("text-viewer"))
            .order(egui::Order::Foreground).open(&mut open).collapsible(false)
            .default_size(egui::vec2(720., 520.).min(maximum)).max_size(maximum)
            .show(ctx, |ui| match &viewer.view {
                View::Loading => { ui.horizontal(|ui| { ui.spinner(); ui.label("Cargando texto…"); }); }
                View::Failed(error) => { ui.colored_label(chat::SEND_FAILED, error); }
                View::Expired => {
                    ui.label("El enlace de este adjunto caducó.");
                    let suspended = state.connection_phase.suspended();
                    if ui.add_enabled(!suspended, egui::Button::new("Actualizar enlace"))
                        .on_disabled_hover_text("Disponible cuando se recupere la conexión.").clicked()
                        && let Source::Remote { channel, message, attachment, .. } = &viewer.source {
                        refresh = Some((channel.clone(), message.clone(), attachment.clone(), viewer.operation));
                    }
                }
                View::Ready(document) => {
                    ui.horizontal(|ui| {
                        if ui.button("Copiar todo").clicked() { ui.ctx().copy_text(document.text.to_string()); }
                        ui.label(RichText::new("Sólo lectura").small().color(self.theme.muted));
                    });
                    ui.separator();
                    let height = ui.text_style_height(&egui::TextStyle::Monospace);
                    autoscroll::ScrollArea::both().id_salt(("text-viewer-rows", viewer.operation))
                        .auto_shrink([false, false])
                        .show_rows(ui, height, document.rows.len(), |ui, range| {
                            for (start, end) in &document.rows[range] {
                                let row = document.text.get(*start as usize..*end as usize).unwrap_or("");
                                ui.add(egui::Label::new(RichText::new(row).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                            }
                        });
                }
            });
        if let Some((channel, message, attachment, operation)) = refresh {
            viewer.view = View::Loading;
            self.send("attachment-refresh", message, "", serde_json::json!({"operation":operation,"attachment":attachment}).to_string(), &channel);
        }
        if !open { self.close_text_viewer(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_unicode_and_boms_but_never_guesses_binary() {
        assert_eq!(decode("añ😀".as_bytes()).unwrap(), "añ😀");
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'a']).unwrap(), "a");
        assert_eq!(decode(&[0xFF, 0xFE, b'a', 0]).unwrap(), "a");
        assert_eq!(decode(&[0xFE, 0xFF, 0, b'a']).unwrap(), "a");
        assert_eq!(decode(b"").unwrap(), "");
        for binary in [&[0xFF, 0xFE, b'a'][..], &[0xFF, 0xFE, 0x00, 0xD8], &[0xC3, 0x28], b"a\0b"] {
            assert!(decode(binary).is_err());
        }
    }
    #[test]
    fn rows_cover_every_byte_of_huge_lines_on_char_boundaries() {
        let text = format!("{}\r\n\nfin", "ñ".repeat(ROW_CHARS * 2 + 1));
        let rows = layout_rows(&text);
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|(start, end)| text.get(*start as usize..*end as usize).is_some()));
        assert_eq!(text[rows[0].0 as usize..rows[0].1 as usize].chars().count(), ROW_CHARS);
        assert_eq!(&text[rows[4].0 as usize..rows[4].1 as usize], "fin");
        assert_eq!(layout_rows(""), vec![(0, 0)]);
    }
    #[test]
    fn input_limit_is_exact_and_sources_are_restricted() {
        let cancel = AtomicBool::new(false);
        assert_eq!(bounded(&vec![b'a'; INPUT_LIMIT][..], &cancel).unwrap().len(), INPUT_LIMIT);
        assert!(bounded(&vec![b'a'; INPUT_LIMIT + 1][..], &cancel).is_err());
        assert!(allowed_source("https://cdn.discordapp.com/attachments/1/2/a.txt?ex=1"));
        for url in ["http://cdn.discordapp.com/a.txt", "https://example.com/a.txt",
            "https://cdn.discordapp.com:8443/a.txt", "https://user@cdn.discordapp.com/a.txt"] {
            assert!(!allowed_source(url));
        }
    }
    #[test]
    fn recognises_declared_text_without_trusting_other_extensions() {
        assert!(text_attachment_name("NOTAS.TXT") && !text_attachment_name("nota.txt.exe"));
        let declared = ChatAttachment { name: "datos".into(), content_type: Some("text/plain; charset=utf-8".into()), ..Default::default() };
        assert!(text_attachment(&declared));
        assert!(!text_attachment(&ChatAttachment { name: "datos.html".into(), content_type: Some("text/html".into()), ..Default::default() }));
    }
}
