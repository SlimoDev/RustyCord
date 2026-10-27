//! Bounded catalog work and the sole per-account picker-history writer.
use super::*;
use crate::pickers::{CatalogRequest, GifPage};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct GifState {
    cache: VecDeque<Cached>,
    jobs: HashMap<String, Job>,
    removals: HashSet<String>,
    revision: u64,
    favorite_epoch: u64,
}
struct Cached {
    mode: String,
    query: String,
    limit: usize,
    at: Instant,
    page: GifPage,
    bytes: usize,
}
struct Job {
    id: String,
    watchers: Vec<CatalogRequest>,
    cancel: CancellationToken,
    started: Instant,
    coalesced: bool,
}
impl GifState {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.cache.is_empty() && self.jobs.is_empty()
    }
    pub(super) fn clear(&mut self) {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
        *self = Self::default();
    }
    fn invalidate_favorites(&mut self) {
        self.cache.retain(|entry| entry.mode != "favorites");
        self.favorite_epoch = self.favorite_epoch.wrapping_add(1);
    }
    fn cached(&mut self, request: &CatalogRequest) -> Option<GifPage> {
        self.cache
            .retain(|entry| entry.at.elapsed() < Duration::from_secs(60));
        let index = self.cache.iter().position(|entry| {
            entry.mode == request.mode
                && entry.query == request.query
                && entry.limit >= request.limit
        })?;
        let entry = self.cache.remove(index)?;
        let mut page = entry.page.clone();
        page.has_more |= page.items.len() > request.limit;
        page.items.truncate(request.limit);
        self.cache.push_back(entry);
        Some(page)
    }
    fn insert(&mut self, request: &CatalogRequest, page: GifPage) {
        let bytes = serde_json::to_vec(&page).map_or(usize::MAX, |value| value.len());
        if bytes > 1024 * 1024 {
            return;
        }
        self.cache.retain(|entry| {
            entry.mode != request.mode
                || entry.query != request.query
                || entry.limit != request.limit
        });
        self.cache.push_back(Cached {
            mode: request.mode.clone(),
            query: request.query.clone(),
            limit: request.limit,
            at: Instant::now(),
            page,
            bytes,
        });
        while self.cache.len() > 8
            || self.cache.iter().map(|entry| entry.bytes).sum::<usize>() > 4 * 1024 * 1024
        {
            self.cache.pop_front();
        }
    }
}
impl Controller {
    fn gif_scope(&self, value: &Value) -> bool {
        s(value, "owner") == self.me()
            && value["generation"].as_u64() == Some(self.state.notification_generation)
    }
    pub(super) fn gif_reply(&self, request: &CatalogRequest, page: Option<GifPage>, error: &str) {
        self.gif_reply_metrics(request, page, error, Value::Null);
    }
    fn gif_reply_metrics(
        &self,
        request: &CatalogRequest,
        page: Option<GifPage>,
        error: &str,
        metrics: Value,
    ) {
        self.data(
            "gif-catalog",
            json!({"stamp":request,"page":page,"error":error,"metrics":metrics}),
        );
    }
    pub(super) fn gif_catalog_action(&mut self, mut action: UiAction) -> Result<(), String> {
        let request: CatalogRequest =
            serde_json::from_str(&action.extra).map_err(|_| "Consulta GIF sin correlación")?;
        let scope = serde_json::to_value(&request).unwrap_or_default();
        if !self.gif_scope(&scope) {
            return Ok(());
        }
        if uuid::Uuid::parse_str(&request.opening).is_err()
            || uuid::Uuid::parse_str(&request.request).is_err()
            || request.query != action.text
            || !(1..=96).contains(&request.limit)
        {
            return Err("Consulta GIF inválida".into());
        }
        let endpoint = pickers::endpoint(&request.mode, &request.query, request.limit)?;
        if request.mode == "favorites" && !self.gif_state.removals.is_empty() {
            self.gif_reply(
                &request,
                None,
                "Se está conciliando un favorito. Vuelve a abrir Guardados al terminar.",
            );
            return Ok(());
        }
        // A newer intent replaces old observers from this opening, even when
        // mode/query changed. Cancel work that no longer has a consumer.
        for job in self.gif_state.jobs.values_mut() {
            job.watchers.retain(|old| old.opening != request.opening);
        }
        if let Some(page) = self.gif_state.cached(&request) {
            for job in self.gif_state.jobs.values() {
                if job.watchers.is_empty() {
                    job.cancel.cancel();
                }
            }
            let metrics = json!({"cache_hit":true,"coalesced_job":false,"job_elapsed_ms":0,"items":page.items.len(),"estimated_cache_bytes":self.gif_state.cache.iter().map(|entry|entry.bytes).sum::<usize>()});
            self.gif_reply_metrics(&request, Some(page), "", metrics);
            return Ok(());
        }
        if let Some(job) = self.gif_state.jobs.get_mut(&endpoint) {
            if job.watchers.len() < 8 {
                job.watchers.push(request);
                job.coalesced = true;
            } else {
                self.gif_reply(
                    &request,
                    None,
                    "Hay demasiadas consultas GIF pendientes. Reintenta.",
                );
            }
            return Ok(());
        }
        for job in self.gif_state.jobs.values() {
            if job.watchers.is_empty() {
                job.cancel.cancel();
            }
        }
        if self.gif_state.jobs.len() >= 4 {
            self.gif_reply(
                &request,
                None,
                "La consulta anterior se está cancelando. Reintenta.",
            );
            return Ok(());
        }
        let api = self.api()?;
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        self.gif_state.jobs.insert(
            endpoint.clone(),
            Job {
                id: id.clone(),
                watchers: vec![request.clone()],
                cancel: cancel.clone(),
                started: Instant::now(),
                coalesced: false,
            },
        );
        action.kind = "__gifs".into();
        action.id = endpoint.clone();
        action.extra =
            json!({"job":id,"stamp":request,"favoriteEpoch":self.gif_state.favorite_epoch})
                .to_string();
        self.launch(action,None,async move{tokio::select!{_=cancel.cancelled()=>Ok(json!({"cancelled":true})),value=api.get(&endpoint)=>value.map_err(|error|error.to_string())}});
        Ok(())
    }
    pub(super) fn cancel_gif_catalog(&mut self, extra: &str) {
        let value: Value = serde_json::from_str(extra).unwrap_or_default();
        if !self.gif_scope(&value) {
            return;
        }
        let opening = s(&value, "opening");
        for job in self.gif_state.jobs.values_mut() {
            job.watchers.retain(|request| request.opening != opening);
            if job.watchers.is_empty() {
                job.cancel.cancel();
            }
        }
    }
    pub(super) fn complete_gif_catalog(
        &mut self,
        action: &UiAction,
        value: &Value,
    ) -> Result<(), String> {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if self
            .gif_state
            .jobs
            .get(&action.id)
            .is_none_or(|job| job.id != s(&extra, "job"))
        {
            return Ok(());
        }
        let job = self.gif_state.jobs.remove(&action.id).unwrap();
        if value["cancelled"].as_bool() == Some(true) {
            for request in job.watchers {
                self.gif_reply(&request, None, "Consulta cancelada. Puedes reintentar.");
            }
            return Ok(());
        }
        let request: CatalogRequest =
            serde_json::from_value(extra["stamp"].clone()).map_err(|_| "Consulta GIF inválida")?;
        if request.mode == "favorites"
            && extra["favoriteEpoch"].as_u64() != Some(self.gif_state.favorite_epoch)
        {
            for request in job.watchers {
                self.gif_reply(
                    &request,
                    None,
                    "Los guardados cambiaron durante la carga. Vuelve a abrirlos.",
                );
            }
            return Ok(());
        }
        let page = match pickers::parse(value, &request.mode, request.limit) {
            Ok(page) => page,
            Err(error) => {
                for request in job.watchers {
                    self.gif_reply(&request, None, &error);
                }
                return Ok(());
            }
        };
        if request.mode == "favorites" {
            self.reconcile_picker_favorites(&s(value, "settings"));
            self.data("gif-favorites-update",json!({"owner":self.me(),"generation":self.state.notification_generation,"page":page}));
        }
        self.gif_state.insert(&request, page.clone());
        let metrics = json!({"cache_hit":false,"coalesced_job":job.coalesced,"job_elapsed_ms":job.started.elapsed().as_millis().min(u64::MAX as u128)as u64,"items":page.items.len(),"estimated_cache_bytes":self.gif_state.cache.iter().map(|entry|entry.bytes).sum::<usize>()});
        for request in job.watchers {
            self.gif_reply_metrics(&request, Some(page.clone()), "", metrics.clone());
        }
        Ok(())
    }
    pub(super) fn gif_failed(&mut self, action: &UiAction) {
        if action.kind == "__gifs" {
            let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
            if self
                .gif_state
                .jobs
                .get(&action.id)
                .is_some_and(|job| job.id == s(&extra, "job"))
                && let Some(job) = self.gif_state.jobs.remove(&action.id)
            {
                for request in job.watchers {
                    self.gif_reply(
                        &request,
                        None,
                        "No se pudo cargar el catálogo. Puedes reintentar.",
                    );
                }
            }
        } else if action.kind == "__gif-remote-remove" {
            self.gif_state.removals.remove(&action.id);
            self.data("gif-favorite-result",json!({"owner":self.me(),"generation":self.state.notification_generation,"url":action.id,"operation":serde_json::from_str::<Value>(&action.extra).unwrap_or_default()["operation"],"confirmed":false,"error":"No se confirmó la eliminación. Se consultará de nuevo antes de otra escritura."}));
        }
    }
    pub(super) fn publish_picker_history(&mut self) {
        self.gif_state.revision = self.gif_state.revision.wrapping_add(1);
        if let Some(history) = &self.picker {
            self.data("picker-history",json!({"owner":self.me(),"generation":self.state.notification_generation,"revision":self.gif_state.revision,"history":history.data}));
        }
    }
    pub(super) fn picker_history_action(&mut self, action: UiAction) -> Result<(), String> {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if !self.gif_scope(&extra) {
            return Ok(());
        }
        let history = self.picker.as_mut().ok_or("Historial no disponible")?;
        if let Some(error) = history.load_error() {
            return Err(format!(
                "El historial local se conserva sin modificar: {error}"
            ));
        }
        match action.kind.as_str() {
            "picker-history" => (),
            "picker-recent" => {
                let value = serde_json::from_value(extra["value"].clone())
                    .map_err(|_| "Elemento no válido")?;
                history.recent(value, extra["gif"].as_bool().unwrap_or(false))?;
            }
            "picker-favorite" => {
                let value = serde_json::from_value(extra["value"].clone())
                    .map_err(|_| "Elemento no válido")?;
                history.set_favorite(
                    value,
                    extra["saved"]
                        .as_bool()
                        .ok_or("Falta la intención de guardado")?,
                )?;
            }
            "picker-import" => {
                let values = serde_json::from_value(extra["values"].clone())
                    .map_err(|_| "Importación no válida")?;
                history.import_values(values)?;
            }
            _ => return Err("Operación de historial inválida".into()),
        }
        self.publish_picker_history();
        Ok(())
    }
    fn reconcile_picker_favorites(&mut self, encoded: &str) {
        if let Some(history) = &mut self.picker
            && history.data.gif_remote_removals.iter().any(|entry| {
                !entry.confirmed
                    || history
                        .data
                        .gif_favorites
                        .iter()
                        .any(|value| value.descriptor().matches_url(&entry.url))
            })
        {
            if let Err(error) = history.reconcile_remote(encoded) {
                self.error(format!(
                    "La lectura de Discord se confirmó; limpieza local pendiente: {error}"
                ));
            }
            self.publish_picker_history();
        }
    }
    pub(super) fn remove_remote_gif(&mut self, mut action: UiAction) -> Result<(), String> {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if !self.gif_scope(&extra) {
            return Ok(());
        }
        if uuid::Uuid::parse_str(&s(&extra, "operation")).is_err() {
            return Err("La intención de favorito no tiene una identidad válida".into());
        }
        if !pickers::safe_link(&action.id) || action.id.len() > 4096 {
            return Err("Favorito GIF inválido".into());
        }
        if !self.gif_state.removals.is_empty() {
            return Err("Espera a que termine la conciliación del favorito anterior".into());
        }
        let api = self.api()?;
        self.picker
            .as_mut()
            .ok_or("Historial no disponible")?
            .prepare_remote_removal(&action.id)?;
        self.gif_state.invalidate_favorites();
        self.gif_state.removals.insert(action.id.clone());
        let url = action.id.clone();
        action.kind = "__gif-remote-remove".into();
        self.launch(action,None,async move{
            let before=api.get("users/@me/settings-proto/2").await.map_err(|error|error.to_string())?;
            let patch=pickers::favorites_wire::remove(&s(&before,"settings"),&url)?;
            let mut out_of_date=false;let mut write_error=false;
            if patch.changed {
                match api.request(Method::PATCH,"users/@me/settings-proto/2",Some(json!({"settings":patch.encoded,"required_data_version":patch.data_version}))).await{
                    Ok(value)=>out_of_date=value["out_of_date"].as_bool().unwrap_or(false),Err(_)=>write_error=true,
                }
            }
            // Never repeat PATCH after timeout/conflict. A fresh canonical read
            // decides whether cleanup is allowed, even after an uncertain write.
            let after=api.get("users/@me/settings-proto/2").await.map_err(|error|error.to_string())?;
            let encoded=after.get("settings").and_then(Value::as_str).ok_or("Discord no entregó un estado canónico de guardados")?;
            let confirmed=!pickers::favorites_wire::contains(encoded,&url)?;
            Ok(json!({"canonical":after,"confirmed":confirmed,"conflict":out_of_date,"uncertain":write_error}))
        });
        Ok(())
    }
    pub(super) fn complete_remote_gif(
        &mut self,
        action: &UiAction,
        value: &Value,
    ) -> Result<(), String> {
        self.gif_state.removals.remove(&action.id);
        self.gif_state.invalidate_favorites();
        let page = pickers::parse(&value["canonical"], "favorites", 96)?;
        self.reconcile_picker_favorites(&s(&value["canonical"], "settings"));
        self.data(
            "gif-favorites-update",
            json!({"owner":self.me(),"generation":self.state.notification_generation,"page":page}),
        );
        let confirmed = value["confirmed"].as_bool().unwrap_or(false);
        let error = if confirmed {
            ""
        } else if value["conflict"].as_bool() == Some(true) {
            "Discord informó un conflicto. Se conservó el estado canónico; vuelve a decidir si quieres eliminarlo."
        } else {
            "No se confirmó la eliminación en Discord. Se conservó el favorito; reintentar comenzará con otra lectura."
        };
        self.data("gif-favorite-result",json!({"owner":self.me(),"generation":self.state.notification_generation,"url":action.id,"operation":serde_json::from_str::<Value>(&action.extra).unwrap_or_default()["operation"],"confirmed":confirmed,"error":error}));
        Ok(())
    }
    pub(super) fn gif_settings_update(&mut self, value: &Value) {
        if i(&value["settings"], "type") != 2 {
            return;
        }
        self.gif_state.invalidate_favorites();
        let encoded = s(&value["settings"], "proto");
        if encoded.is_empty() {
            return;
        }
        if b(value, "partial") && !pickers::favorites_wire::has_favorites(&encoded).unwrap_or(false)
        {
            return;
        }
        if let Ok(page) = pickers::parse(&json!({"settings":encoded}), "favorites", 96) {
            self.reconcile_picker_favorites(&encoded);
            self.data("gif-favorites-update",json!({"owner":self.me(),"generation":self.state.notification_generation,"page":page}));
        }
    }
}
