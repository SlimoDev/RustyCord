//! Public, unauthenticated update metadata. Downloads are explicit and never use
//! Discord credentials. The installed, compiled manifest fixes the update lane.
use eframe::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        OnceLock,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

const INSTALLED: &str = include_str!("../../../version.json");
const INSTALLER: &str = include_str!("../../../scripts/install-update.ps1");
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub schema: u32,
    pub variant: String,
    pub branch: String,
    pub version: String,
    pub release: Option<Release>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

fn installed() -> &'static Manifest {
    static VALUE: OnceLock<Manifest> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(INSTALLED).expect("version.json debe ser válido al compilar")
    })
}
pub fn installed_label() -> String {
    format!("{} {}", crate::brand::NAME, installed().version)
}
pub fn installed_version() -> String {
    installed().version.clone()
}
pub fn source_branch() -> String {
    installed().branch.clone()
}

#[derive(Clone, Debug)]
pub enum Status {
    Idle,
    Checking,
    Current,
    Available { version: String, downloadable: bool },
    Downloading { received: u64, total: u64 },
    Preparing,
    Ready,
    Error(String),
}
enum Event {
    Checked(Result<Option<Manifest>, String>),
    Progress(u64, u64),
    Preparing,
    Prepared(Result<PathBuf, String>),
}

pub struct UpdateManager {
    pub status: Status,
    manifest: Option<Manifest>,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    last_check: Option<Instant>,
    started: Instant,
    busy: bool,
    shutdown: bool,
    prepared_plan: Option<PathBuf>,
}
impl Default for UpdateManager {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            status: Status::Idle,
            manifest: None,
            rx,
            tx,
            last_check: None,
            started: Instant::now(),
            busy: false,
            shutdown: false,
            prepared_plan: None,
        }
    }
}
impl UpdateManager {
    /// Call once per root frame. Preview/fixture views pass false, so they never
    /// contact GitHub. One result replaces the footer; there are no repeated toasts.
    pub fn poll(&mut self, ctx: &egui::Context, allow_network: bool) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Checked(result) => {
                    self.busy = false;
                    match result {
                        Ok(Some(manifest)) => {
                            self.status = Status::Available {
                                version: manifest.version.clone(),
                                downloadable: manifest.release.is_some(),
                            };
                            self.manifest = Some(manifest);
                        }
                        Ok(None) => {
                            self.manifest = None;
                            self.status = Status::Current;
                        }
                        Err(error) => self.status = Status::Error(error),
                    }
                }
                Event::Progress(received, total) => {
                    self.status = Status::Downloading { received, total }
                }
                Event::Preparing => self.status = Status::Preparing,
                Event::Prepared(result) => {
                    self.busy = false;
                    match result {
                        Ok(plan) => {
                            self.prepared_plan = Some(plan);
                            self.status = Status::Ready;
                            self.shutdown = true;
                        }
                        Err(error) => self.status = Status::Error(error),
                    }
                }
            }
        }
        if allow_network
            && !self.busy
            && self.started.elapsed() >= Duration::from_secs(5)
            && self
                .last_check
                .is_none_or(|last| last.elapsed() >= CHECK_INTERVAL)
        {
            self.check(ctx);
        }
        if allow_network && !self.busy {
            let remaining = self
                .last_check
                .map(|last| CHECK_INTERVAL.saturating_sub(last.elapsed()))
                .unwrap_or_else(|| Duration::from_secs(5).saturating_sub(self.started.elapsed()));
            ctx.request_repaint_after(remaining.max(Duration::from_secs(1)));
        }
    }
    pub fn take_shutdown_request(&mut self) -> bool {
        std::mem::take(&mut self.shutdown)
    }
    /// A configuration save failed before the UI could finish the handoff. The
    /// helper must release Admission immediately instead of holding all future
    /// launches behind its normal process-exit timeout.
    pub fn cancel_prepared(&mut self, error: impl Into<String>) {
        let mut error = error.into();
        if let Some(plan) = self.prepared_plan.take() {
            if plan.parent().is_none_or(|work| {
                fs::write(work.join("cancel-install"), b"configuration-save-failed").is_err()
            }) {
                error.push_str(
                    " No se pudo avisar al instalador; su espera terminará automáticamente.",
                );
            }
        }
        self.busy = false;
        self.shutdown = false;
        self.status = Status::Error(error);
    }
    pub fn check(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = Status::Checking;
        self.last_check = Some(Instant::now());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Checked(check_remote()));
            ctx.request_repaint();
        });
    }
    pub fn start_update(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let Some(manifest) = self.manifest.clone().filter(|m| m.release.is_some()) else {
            return;
        };
        self.busy = true;
        self.status = Status::Downloading {
            received: 0,
            total: manifest.release.as_ref().map_or(0, |r| r.bytes),
        };
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = prepare_update(&manifest, &tx, &ctx);
            let _ = tx.send(Event::Prepared(result));
            ctx.request_repaint();
        });
    }
    /// Shared footer control. The surrounding sidebar reserves its own height.
    pub fn button(&mut self, ui: &mut egui::Ui) {
        let (enabled, label, hint) = match &self.status {
            Status::Idle => (
                false,
                "Actualizar".into(),
                "Consultar la versión publicada".into(),
            ),
            Status::Checking => (
                false,
                "Consultando…".into(),
                "Consultando GitHub sin iniciar sesión".into(),
            ),
            Status::Current => (
                false,
                "Actualizar".into(),
                format!(
                    "{} · al día. Usa ↻ para consultar de nuevo",
                    installed_label()
                ),
            ),
            Status::Available {
                version,
                downloadable,
            } => (
                *downloadable,
                "Actualizar".into(),
                if *downloadable {
                    format!(
                        "Instalar {version} desde la rama {}. La aplicación se cerrará y abrirá de nuevo",
                        source_branch()
                    )
                } else {
                    format!(
                        "Versión {version} publicada; su paquete de instalación aún no está disponible"
                    )
                },
            ),
            Status::Downloading { received, total } => (
                false,
                format!(
                    "Descargando {} %",
                    received.saturating_mul(100) / (*total).max(1)
                ),
                "La llamada continúa hasta que el paquete esté preparado".into(),
            ),
            Status::Preparing => (
                false,
                "Preparando…".into(),
                "Comprobando el paquete antes de cerrar".into(),
            ),
            Status::Ready => (
                false,
                "Reiniciando…".into(),
                "Guardando cambios y cerrando conexiones".into(),
            ),
            Status::Error(error) => (false, "Actualizar".into(), error.clone()),
        };
        ui.horizontal(|ui| {
            let response = ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(&hint)
                .on_disabled_hover_text(&hint);
            if response.clicked() {
                self.start_update(ui.ctx());
            }
            if ui
                .add_enabled(
                    !self.busy && !matches!(self.status, Status::Ready),
                    egui::Button::new("↻"),
                )
                .on_hover_text("Consultar o reintentar la búsqueda de actualizaciones")
                .clicked()
            {
                self.check(ui.ctx());
            }
        });
        match &self.status {
            Status::Available {
                version,
                downloadable,
            } => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if *downloadable {
                            format!("Disponible: {version}")
                        } else {
                            format!("{version}: paquete pendiente")
                        })
                        .small(),
                    )
                    .truncate(),
                );
            }
            Status::Error(error) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new("Error de actualización · reintenta ↻").small(),
                    )
                    .truncate(),
                )
                .on_hover_text(error);
            }
            _ => {}
        }
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15 * 60))
        .user_agent("NotDiscord-Update/1")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5
                || url.scheme() != "https"
                || !matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "raw.githubusercontent.com"
                    )
                )
            {
                attempt.error("Destino de actualización no permitido")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| "No se pudo preparar la conexión de actualización".into())
}
fn lane_valid(manifest: &Manifest) -> bool {
    manifest.schema == 1
        && matches!(
            (manifest.variant.as_str(), manifest.branch.as_str()),
            ("standard", "NotDiscord") | ("plus", "codex/notdiscord-plus")
        )
}
fn check_remote() -> Result<Option<Manifest>, String> {
    let own = installed();
    if !lane_valid(&own) {
        return Err("La variante instalada no tiene una rama de actualización válida".into());
    }
    let mut url = url::Url::parse(
        "https://api.github.com/repos/SlimoDev/RustyCord/contents/NotDiscord-Rust/version.json",
    )
    .unwrap();
    url.query_pairs_mut().append_pair("ref", &own.branch);
    let mut response = client()?
        .get(url)
        .header("Accept", "application/vnd.github.raw+json")
        .timeout(Duration::from_secs(25))
        .send()
        .map_err(|_| "No se pudo consultar GitHub; puedes reintentar".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub respondió {} al consultar la versión",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|n| n > 64 * 1024) {
        return Err("El manifiesto de actualización es demasiado grande".into());
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "No se pudo leer el manifiesto".to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("El manifiesto de actualización es demasiado grande".into());
    }
    let remote: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| "GitHub no publicó un manifiesto de versión válido".to_string())?;
    if !lane_valid(&remote) || remote.variant != own.variant || remote.branch != own.branch {
        return Err("El manifiesto pertenece a otra variante".into());
    }
    let current = Version::parse(&own.version)?;
    let newer = Version::parse(&remote.version)?;
    if let Some(release) = &remote.release {
        validate_release(&remote, release)?;
    }
    Ok((newer > current).then_some(remote))
}
fn validate_release(manifest: &Manifest, release: &Release) -> Result<(), String> {
    let valid_tag = !release.tag.is_empty()
        && release.tag.len() <= 100
        && release
            .tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._+".contains(&b));
    let prefix = if manifest.variant == "plus" {
        "NotDiscord-Plus"
    } else {
        "NotDiscord"
    };
    let expected = format!(
        "https://github.com/SlimoDev/RustyCord/releases/download/{}/{}-{}.zip",
        release.tag, prefix, manifest.version
    );
    if !valid_tag
        || release.url != expected
        || release.bytes == 0
        || release.bytes > MAX_PACKAGE_BYTES
        || release.sha256.len() != 64
        || !release.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("El paquete publicado tiene una ruta, tamaño o SHA-256 no válido".into());
    }
    Ok(())
}
#[derive(Serialize)]
struct InstallPlan {
    schema: u32,
    target_root: PathBuf,
    archive: PathBuf,
    work_root: PathBuf,
    variant: String,
    branch: String,
    version: String,
    sha256: String,
    bytes: u64,
    parent_pid: u32,
    executable: PathBuf,
    arguments: String,
    admission_mutex: String,
    owner_mutex: String,
}
fn prepare_update(
    manifest: &Manifest,
    tx: &Sender<Event>,
    ctx: &egui::Context,
) -> Result<PathBuf, String> {
    let release = manifest
        .release
        .as_ref()
        .ok_or("La versión aún no tiene paquete")?;
    validate_release(manifest, release)?;
    let executable = std::env::current_exe().map_err(|_| "No se encontró el ejecutable actual")?;
    let target = executable
        .parent()
        .ok_or("No se encontró la carpeta instalada")?;
    if !target.join("runtime.json").is_file() {
        return Err("Esta ejecución procede del código fuente. Instala un paquete compilado para actualizar desde la aplicación".into());
    }
    reject_reparse_ancestors(target)?;
    let base = target.join(".notdiscord-update");
    if base.exists() {
        reject_reparse_ancestors(&base)?;
    }
    fs::create_dir_all(&base)
        .map_err(|_| "La carpeta instalada no permite preparar la actualización")?;
    let work = base.join(uuid::Uuid::new_v4().simple().to_string());
    fs::create_dir(&work).map_err(|_| "No se pudo crear la carpeta temporal de actualización")?;
    let archive = work.join("package.zip");
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&archive)
        .map_err(|_| "No se pudo preparar la descarga")?;
    let mut response = client()?
        .get(&release.url)
        .send()
        .map_err(|_| "No se pudo descargar la actualización")?;
    if !response.status().is_success() {
        return Err(format!(
            "La descarga respondió {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|n| n != release.bytes)
    {
        return Err("El tamaño publicado no coincide con la descarga".into());
    }
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    let mut progress = Instant::now();
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|_| "La descarga se interrumpió; puedes reintentar")?;
        if count == 0 {
            break;
        }
        received = received
            .checked_add(count as u64)
            .ok_or("Tamaño de descarga no válido")?;
        if received > release.bytes {
            return Err("La descarga supera el tamaño publicado".into());
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| "No se pudo guardar la descarga")?;
        if progress.elapsed() >= Duration::from_millis(150) {
            let _ = tx.send(Event::Progress(received, release.bytes));
            ctx.request_repaint();
            progress = Instant::now();
        }
    }
    output
        .sync_all()
        .map_err(|_| "No se pudo finalizar la descarga")?;
    drop(output);
    if received != release.bytes
        || format!("{:x}", hash.finalize()) != release.sha256.to_ascii_lowercase()
    {
        return Err("El tamaño o SHA-256 de la descarga no coincide; no se instalará".into());
    }
    let (admission_mutex, owner_mutex) = crate::instance::coordination_names()?;
    let plan = InstallPlan {
        schema: 1,
        target_root: target.to_path_buf(),
        archive,
        work_root: work.clone(),
        variant: manifest.variant.clone(),
        branch: manifest.branch.clone(),
        version: manifest.version.clone(),
        sha256: release.sha256.clone(),
        bytes: release.bytes,
        parent_pid: std::process::id(),
        executable: executable.clone(),
        arguments: std::env::args_os()
            .skip(1)
            .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" "),
        admission_mutex,
        owner_mutex,
    };
    let plan_path = work.join("plan.json");
    fs::write(
        &plan_path,
        serde_json::to_vec(&plan).map_err(|_| "No se pudo crear el plan de actualización")?,
    )
    .map_err(|_| "No se pudo guardar el plan de actualización")?;
    // Windows PowerShell 5 reads BOM-less source as the legacy ANSI codepage.
    let mut installer = vec![0xef, 0xbb, 0xbf];
    installer.extend_from_slice(INSTALLER.as_bytes());
    fs::write(work.join("install-update.ps1"), installer)
        .map_err(|_| "No se pudo preparar el instalador")?;
    let _ = tx.send(Event::Preparing);
    ctx.request_repaint();
    let output = installer_command(&plan_path, "Prepare")?
        .output()
        .map_err(|_| "No se pudo preparar el instalador de Windows")?;
    if !output.status.success() {
        return Err("El paquete no pasó la validación previa. Consulta .notdiscord-update en la carpeta instalada".into());
    }
    launch_installer(&plan_path)?;
    Ok(plan_path)
}
fn installer_command(plan: &Path, mode: &str) -> Result<Command, String> {
    let parent = plan
        .parent()
        .ok_or("No se encontró el plan de actualización")?;
    let system =
        std::env::var_os("SystemRoot").ok_or("Windows no informó su carpeta de sistema")?;
    let mut command =
        Command::new(PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(parent.join("install-update.ps1"))
        .args(["-Mode", mode, "-PlanPath"])
        .arg(plan);
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    Ok(command)
}
fn launch_installer(plan: &Path) -> Result<(), String> {
    let mut child = installer_command(plan, "Install")?
        .spawn()
        .map_err(|_| "No se pudo iniciar la instalación; la aplicación seguirá abierta")?;
    let ready = plan
        .parent()
        .ok_or("No se encontró el plan de actualización")?
        .join("installer-ready");
    let waiting = Instant::now();
    while waiting.elapsed() < Duration::from_secs(90) {
        if ready.is_file() {
            fs::write(
                ready.with_file_name("commit-install"),
                b"user-requested-update",
            )
            .map_err(|_| "No se pudo confirmar el cierre para actualizar")?;
            return Ok(());
        }
        if child
            .try_wait()
            .map_err(|_| "No se pudo comprobar el instalador")?
            .is_some()
        {
            return Err("El instalador no pudo prepararse. La aplicación seguirá abierta; consulta .notdiscord-update".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Without an acknowledgement the UI must stay open. The helper expires its
    // own 120-second wait without modifying the running installation.
    Err("El instalador no confirmó que esté preparado; no se cerrará la aplicación".into())
}
fn reject_reparse_ancestors(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    for current in path.ancestors() {
        if fs::symlink_metadata(current)
            .map_err(|_| "No se pudo comprobar la carpeta instalada")?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err(
                "La actualización requiere una carpeta instalada sin enlaces de directorio".into(),
            );
        }
    }
    Ok(())
}
fn quote_windows_arg(text: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in text.chars() {
        if ch == '\\' {
            slashes += 1;
        } else {
            result.extend(std::iter::repeat_n(
                '\\',
                if ch == '\"' { slashes * 2 + 1 } else { slashes },
            ));
            result.push(ch);
            slashes = 0;
        }
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

// SemVer precedence including numeric prerelease identifiers; build metadata is
// ignored. Parsing fails closed instead of sorting version labels alphabetically.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<String>,
}
impl Version {
    fn parse(text: &str) -> Result<Self, String> {
        if text.is_empty() || text.len() > 100 {
            return Err("La versión publicada no es SemVer".into());
        }
        let (precedence, build) = text
            .split_once('+')
            .map_or((text, None), |(a, b)| (a, Some(b)));
        let (core, pre) = precedence
            .split_once('-')
            .map_or((precedence, None), |(a, b)| (a, Some(b)));
        let ids = |value: &str, numeric_strict: bool| -> Result<Vec<String>, String> {
            value
                .split('.')
                .map(|id| {
                    if id.is_empty()
                        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        || (numeric_strict
                            && id.bytes().all(|b| b.is_ascii_digit())
                            && id.len() > 1
                            && id.starts_with('0'))
                    {
                        Err("La versión publicada no es SemVer".into())
                    } else {
                        Ok(id.into())
                    }
                })
                .collect()
        };
        if let Some(build) = build {
            let _ = ids(build, false)?;
        }
        let parts = core.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err("La versión publicada debe incluir mayor.menor.parche".into());
        }
        let number = |part: &str| -> Result<u64, String> {
            if part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                Err("La versión publicada no es SemVer".into())
            } else {
                part.parse()
                    .map_err(|_| "Número de versión fuera de rango".into())
            }
        };
        Ok(Self {
            major: number(parts[0])?,
            minor: number(parts[1])?,
            patch: number(parts[2])?,
            pre: pre.map(|v| ids(v, true)).transpose()?.unwrap_or_default(),
        })
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let an = a.bytes().all(|v| v.is_ascii_digit());
            let bn = b.bytes().all(|v| v.is_ascii_digit());
            let result = match (an, bn) {
                (true, true) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => a.cmp(b),
            };
            if result != Ordering::Equal {
                return result;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
