//! Source-preserving timestamp presentation and a scoped, local-only editor.
use super::*;
use chrono::{Datelike, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use notdiscord_core::message_atoms::Timestamp as MessageTimestamp;

#[repr(C)]
struct SystemTime {
    year:u16,month:u16,weekday:u16,day:u16,hour:u16,minute:u16,second:u16,millis:u16,
}
#[link(name="kernel32")]
unsafe extern "system" {
    fn GetDateFormatEx(locale:*const u16,flags:u32,date:*const SystemTime,format:*const u16,
        output:*mut u16,length:i32,calendar:*const u16)->i32;
    fn GetTimeFormatEx(locale:*const u16,flags:u32,time:*const SystemTime,format:*const u16,
        output:*mut u16,length:i32)->i32;
    fn GetLocaleInfoEx(locale:*const u16,kind:u32,output:*mut u16,length:i32)->i32;
    fn LoadLibraryExW(name:*const u16,file:*mut std::ffi::c_void,flags:u32)->*mut std::ffi::c_void;
    fn GetProcAddress(module:*mut std::ffi::c_void,name:*const u8)->*mut std::ffi::c_void;
    fn FreeLibrary(module:*mut std::ffi::c_void)->i32;
}
type RelativeOpen=unsafe extern "C" fn(*const std::ffi::c_char,*mut std::ffi::c_void,i32,i32,*mut i32)->*mut std::ffi::c_void;
type RelativeFormat=unsafe extern "C" fn(*const std::ffi::c_void,f64,i32,*mut u16,i32,*mut i32)->i32;
type RelativeClose=unsafe extern "C" fn(*mut std::ffi::c_void);
struct NativeRelative {open:RelativeOpen,format:RelativeFormat,close:RelativeClose}
fn native_relative(quantity:u64,unit:usize,future:bool)->Option<String> {
    static FUNCTIONS:std::sync::OnceLock<Option<NativeRelative>>=std::sync::OnceLock::new();
    let functions=FUNCTIONS.get_or_init(|| {
        // Windows' own ICU, optionally loaded from System32 only. Never search
        // the working directory or ship a second ICU runtime in the installer.
        let name="icu.dll".encode_utf16().chain([0]).collect::<Vec<_>>();
        let module=unsafe {LoadLibraryExW(name.as_ptr(),std::ptr::null_mut(),0x800)};
        if module.is_null() {return None;}
        let open=unsafe {GetProcAddress(module,c"ureldatefmt_open".as_ptr().cast())};
        let format=unsafe {GetProcAddress(module,c"ureldatefmt_formatNumeric".as_ptr().cast())};
        let close=unsafe {GetProcAddress(module,c"ureldatefmt_close".as_ptr().cast())};
        if open.is_null() || format.is_null() || close.is_null() {unsafe {FreeLibrary(module)};return None;}
        // SDK icu.h declares these exact C signatures (ICU57+). The one module
        // reference intentionally lives until process exit with these pointers.
        Some(NativeRelative {open:unsafe {std::mem::transmute::<*mut std::ffi::c_void,RelativeOpen>(open)},
            format:unsafe {std::mem::transmute::<*mut std::ffi::c_void,RelativeFormat>(format)},
            close:unsafe {std::mem::transmute::<*mut std::ffi::c_void,RelativeClose>(close)}})
    }).as_ref()?;
    let locale=std::ffi::CString::new(locale_info(0x5c)?).ok()?;
    let mut status=0;let mut output=[0_u16;256];
    let formatter=unsafe {(functions.open)(locale.as_ptr(),std::ptr::null_mut(),0,0x100,&mut status)};
    if formatter.is_null() {return None;}
    if status>0 {unsafe {(functions.close)(formatter)};return None;}
    let offset=quantity as f64*if future{1.}else{-1.};
    let unit=[7,6,5,4,2,0][unit]; // SECOND, MINUTE, HOUR, DAY, MONTH, YEAR
    let length=unsafe {(functions.format)(formatter,offset,unit,output.as_mut_ptr(),output.len() as i32,&mut status)};
    unsafe {(functions.close)(formatter)};
    if status>0 || length<=0 || length as usize>output.len() {return None;}
    String::from_utf16(&output[..length as usize]).ok()
}
fn locale_info(kind:u32)->Option<String> {
    let mut buffer=[0_u16;256];
    let length=unsafe {GetLocaleInfoEx(std::ptr::null(),kind,buffer.as_mut_ptr(),buffer.len() as i32)};
    (length>1 && length as usize<=buffer.len()).then(||String::from_utf16(&buffer[..length as usize-1]).ok()).flatten()
}
fn system_time(value:&chrono::DateTime<Local>)->SystemTime {
    SystemTime {year:value.year() as u16,month:value.month() as u16,
        weekday:value.weekday().num_days_from_sunday() as u16,day:value.day() as u16,
        hour:value.hour() as u16,minute:value.minute() as u16,second:value.second() as u16,millis:0}
}
/// Remove the weekday field (and its following separators) from the user's
/// long-date pattern for Discord's `f`, keeping quoted literals intact.
fn without_weekday(pattern:&str)->String {
    let chars=pattern.chars().collect::<Vec<_>>();let mut result=String::new();let mut i=0;let mut quoted=false;
    while i<chars.len() {
        if chars[i]=='\'' {
            result.push(chars[i]);i+=1;
            if i<chars.len() && chars[i]=='\'' {result.push(chars[i]);i+=1;}else{quoted=!quoted;}
        }else if !quoted && chars[i]=='d' {
            let start=i;while i<chars.len() && chars[i]=='d' {i+=1;}
            if i-start>=3 {
                while i<chars.len() && (chars[i].is_whitespace()||matches!(chars[i],','|'.'|'-'|'/'|'،'|'、')) {i+=1;}
                while result.chars().last().is_some_and(|ch|ch.is_whitespace()||matches!(ch,','|'.'|'-'|'/'|'،'|'、')) {result.pop();}
                if !result.is_empty() && i<chars.len() {result.push(' ');}
            }else{for _ in start..i {result.push('d');}}
        }else{result.push(chars[i]);i+=1;}
    }
    result.trim().to_owned()
}
fn native_date(value:&chrono::DateTime<Local>,long:bool,weekday:bool)->String {
    let date=system_time(value);let mut buffer=[0_u16;256];
    let pattern=if long && !weekday {locale_info(0x20).map(|value|without_weekday(&value))}else{None};
    let pattern=pattern.filter(|value|!value.is_empty()).map(|value|value.encode_utf16().chain([0]).collect::<Vec<_>>());
    let flags=if pattern.is_some(){0}else if long {2}else{1};
    let length=unsafe {GetDateFormatEx(std::ptr::null(),flags,&date,pattern.as_ref().map_or(std::ptr::null(),|value|value.as_ptr()),
        buffer.as_mut_ptr(),buffer.len() as i32,std::ptr::null())};
    if length>0 && length as usize<=buffer.len() {
        if let Ok(text)=String::from_utf16(&buffer[..length as usize-1]) {return text;}
    }
    value.format("%Y-%m-%d").to_string()
}
fn native_time(value:&chrono::DateTime<Local>,seconds:bool)->String {
    let time=system_time(value);let mut buffer=[0_u16;256];
    let length=unsafe {GetTimeFormatEx(std::ptr::null(),if seconds{0}else{2},&time,std::ptr::null(),buffer.as_mut_ptr(),buffer.len() as i32)};
    if length>0 && length as usize<=buffer.len() {
        if let Ok(text)=String::from_utf16(&buffer[..length as usize-1]) {return text;}
    }
    value.format(if seconds{"%H:%M:%S"}else{"%H:%M"}).to_string()
}
fn relative_parts(seconds:i64,now:i64)->(u64,usize,Duration) {
    let delta=seconds.saturating_sub(now);let distance=delta.unsigned_abs();
    let (quantity,unit,refresh)=if distance<60 {(distance,0,1)}else if distance<3600 {(distance/60,1,30)}
        else if distance<86_400 {(distance/3600,2,60)}else if distance<2_592_000 {(distance/86_400,3,300)}
        else if distance<31_536_000 {(distance/2_592_000,4,1800)}else{(distance/31_536_000,5,3600)};
    (quantity,unit,Duration::from_secs(refresh))
}
fn relative(seconds:i64,now:i64)->(String,Duration) {
    let delta=seconds.saturating_sub(now);let distance=delta.unsigned_abs();
    let (quantity,unit,refresh)=relative_parts(seconds,now);
    if let Some(text)=native_relative(quantity,unit,delta>=0) {return (text,refresh);}
    let language=locale_info(0x59).unwrap_or_default();
    let text=if language=="es" {
        let units=[("segundo","segundos"),("minuto","minutos"),("hora","horas"),("día","días"),("mes","meses"),("año","años")];
        let label=if quantity==1 {units[unit].0}else{units[unit].1};
        if distance<1 {"ahora".into()}else if delta<0 {format!("hace {quantity} {label}")}else{format!("dentro de {quantity} {label}")}
    }else if language=="en" {
        let units=["second","minute","hour","day","month","year"];let plural=if quantity==1{""}else{"s"};
        if distance<1 {"now".into()}else if delta<0 {format!("{quantity} {}{plural} ago",units[unit])}else{format!("in {quantity} {}{plural}",units[unit])}
    }else {
        // Locale-neutral duration fallback: never impose Spanish/English date
        // order or a foreign relative sentence on another Windows language.
        let (prefix,suffix)=[("PT","S"),("PT","M"),("PT","H"),("P","D"),("P","M"),("P","Y")][unit];
        format!("{}{prefix}{quantity}{suffix}",if delta<0{"−"}else{"+"})
    };
    (text,refresh)
}
fn absolute(stamp:MessageTimestamp)->Option<String> {
    let value=chrono::DateTime::from_timestamp(stamp.seconds,0)?.with_timezone(&Local);
    Some(match stamp.format {
        't'=>native_time(&value,false),'T'=>native_time(&value,true),
        'd'=>native_date(&value,false,false),'D'=>native_date(&value,true,true),
        's'=>format!("{} {}",native_date(&value,false,false),native_time(&value,false)),
        'S'=>format!("{} {}",native_date(&value,false,false),native_time(&value,true)),
        'F'=>format!("{} {}",native_date(&value,true,true),native_time(&value,false)),
        _=>format!("{} {}",native_date(&value,true,false),native_time(&value,false)),
    })
}
/// Shared snapshot projection for notifications and compact message previews.
/// It performs no lookup or mutation; relative text uses the current clock.
pub(crate) fn display(stamp:MessageTimestamp)->String {
    if stamp.format=='R' {relative(stamp.seconds,chrono::Utc::now().timestamp()).0}
    else {absolute(stamp).unwrap_or_else(||format!("<t:{}:{}>",stamp.seconds,stamp.format))}
}
fn tooltip(stamp:MessageTimestamp)->String {
    let Some(value)=chrono::DateTime::from_timestamp(stamp.seconds,0).map(|value|value.with_timezone(&Local)) else{return String::new();};
    format!("{} {} · UTC{}",native_date(&value,true,true),native_time(&value,true),value.format("%:z"))
}

#[derive(Clone)]
struct Formatted {at:Instant,text:String,tooltip:String}
#[derive(Clone,Default)]
struct FormatCache {items:HashMap<(i64,char,i32),Formatted>}

pub(super) struct Picker {
    pub(super) open_id:String,
    account:String,generation:u64,channel:String,revision:u64,start:usize,end:usize,
    expected:Option<String>,date:String,time:String,format:char,occurrence:Option<bool>,error:String,
}
impl Picker {
    fn timestamp(&self)->Result<MessageTimestamp,String> {
        let date=NaiveDate::parse_from_str(&self.date,"%Y-%m-%d").map_err(|_|"Escribe una fecha válida: AAAA-MM-DD.".to_string())?;
        if !(1..=9999).contains(&date.year()) {return Err("El año debe estar entre 1 y 9999.".into());}
        let time=NaiveTime::parse_from_str(&self.time,"%H:%M:%S").or_else(|_|NaiveTime::parse_from_str(&self.time,"%H:%M"))
            .map_err(|_|"Escribe una hora válida: HH:MM o HH:MM:SS.".to_string())?;
        let value=match Local.from_local_datetime(&date.and_time(time)) {
            chrono::LocalResult::Single(value)=>value,
            chrono::LocalResult::Ambiguous(first,second)=>match self.occurrence {Some(false)=>first,Some(true)=>second,
                None=>return Err("Esta hora ocurre dos veces por el cambio de horario. Elige una ocurrencia.".into())},
            chrono::LocalResult::None=>return Err("Esta hora no existe en la zona horaria de Windows por un cambio de horario.".into()),
        };
        let stamp=MessageTimestamp {seconds:value.timestamp(),format:self.format};
        notdiscord_core::message_atoms::token(&format!("<t:{}:{}>",stamp.seconds,stamp.format))
            .map(|_|stamp).ok_or_else(||"La fecha queda fuera del intervalo admitido.".into())
    }
}
impl NotDiscordUi {
    pub(super) fn timestamp_label(&mut self,ui:&mut egui::Ui,stamp:MessageTimestamp,source:&str,size:f32,
        style:notdiscord_core::markdown::Style)->egui::Response {
        let offset=Local::now().offset().local_minus_utc();let key=(stamp.seconds,stamp.format,offset);
        let cache_id=egui::Id::new("timestamp-display-cache");
        let ttl=if stamp.format=='R' {relative_parts(stamp.seconds,chrono::Utc::now().timestamp()).2}else{Duration::from_secs(5)};
        let cached=ui.ctx().data_mut(|data|data.get_temp_mut_or_default::<FormatCache>(cache_id)
            .items.get(&key).filter(|value|value.at.elapsed()<ttl.min(Duration::from_secs(5))).cloned());
        let formatted=if let Some(cached)=cached {cached}else{
            let value=Formatted {at:Instant::now(),text:display(stamp),tooltip:tooltip(stamp)};
            ui.ctx().data_mut(|data| {
                let cache=data.get_temp_mut_or_default::<FormatCache>(cache_id);
                if cache.items.len()>=256 {
                    if let Some(old)=cache.items.iter().min_by_key(|(_,value)|value.at).map(|(key,_)|*key) {cache.items.remove(&old);}
                }
                cache.items.insert(key,value.clone());
            });value
        };
        let text=theme::markdown_text(&formatted.text,size,self.theme.ink,style);
        let (position,galley,response)=egui::Label::new(text).selectable(true).layout_in_ui(ui);
        ui.painter().rect_filled(response.rect,2.,self.theme.selected);
        if !super::chat::register_history_atom(ui,&response,position,galley.clone(),source) {
            egui::text_selection::LabelSelectionState::label_text_selection(ui,&response,position,galley,
                self.theme.ink,egui::Stroke::NONE);
        }
        if stamp.format=='R' && ui.is_rect_visible(response.rect) {ui.ctx().request_repaint_after(ttl.saturating_sub(formatted.at.elapsed()));}
        response.on_hover_cursor(egui::CursorIcon::Text).on_hover_text(formatted.tooltip)
    }
    pub(super) fn open_timestamp_picker(&mut self,state:&AppSnapshot,start:usize,end:usize,revision:u64) {
        let Some(account)=state.current_user.as_ref().filter(|_|state.signed_in && state.view=="chat" && state.channel_access.can_send) else{return;};
        if start>end || end.saturating_sub(start)>101 {return;}
        let now=Local::now();
        self.time_picker=Some(Picker {open_id:uuid::Uuid::new_v4().simple().to_string(),account:account.id.clone(),
            generation:state.notification_generation,channel:state.selected_channel_id.clone(),revision,start,end,expected:None,
            date:now.format("%Y-%m-%d").to_string(),time:now.format("%H:%M:%S").to_string(),format:'f',occurrence:None,error:String::new()});
        self.mention_closed=true;self.ctx.request_repaint();
    }
    pub(super) fn draw_timestamp_picker(&mut self,ctx:&egui::Context,state:&AppSnapshot) {
        let Some(mut picker)=self.time_picker.take() else{return;};
        let matches=state.signed_in && state.view=="chat" && state.channel_access.can_send && state.notification_generation==picker.generation
            && state.current_user.as_ref().is_some_and(|user|user.id==picker.account) && state.selected_channel_id==picker.channel;
        if !matches {return;}
        let Some(draft)=self.drafts.get(&picker.channel).filter(|draft|draft.revision==picker.revision) else{return;};
        let selection=draft.text.chars().skip(picker.start).take(picker.end-picker.start).collect::<String>();
        if draft.text.chars().count()<picker.end || !selection.starts_with('@') {return;}
        if picker.expected.as_ref().is_some_and(|expected|expected!=&selection) {return;}
        picker.expected.get_or_insert(selection);
        let mut open=true;let mut confirm=false;let mut cancel=false;
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|input|input.consume_key(egui::Modifiers::NONE,egui::Key::Escape)) {open=false;}
        if open {
            super::dialogs::centered_dialog(ctx,"Fecha y hora","timestamp-picker",egui::vec2(510.,420.)).open(&mut open).show(ctx,|ui| {
                ui.label("Se usará la zona horaria de Windows. Cada persona verá la fecha en su propia zona.");ui.separator();
                ui.label("Fecha · AAAA-MM-DD");
                ui.add(bordered(egui::TextEdit::singleline(&mut picker.date).char_limit(10).id(egui::Id::new("timestamp-date")))
                    .focus_id(egui::Id::new("timestamp-date")).submit_action("timestamp-confirm"));
                ui.label("Hora · HH:MM:SS");
                ui.add(bordered(egui::TextEdit::singleline(&mut picker.time).char_limit(8).id(egui::Id::new("timestamp-time")))
                    .focus_id(egui::Id::new("timestamp-time")).submit_action("timestamp-confirm"));
                egui::ComboBox::from_id_salt("timestamp-format").selected_text(format_name(picker.format)).show_ui(ui,|ui| {
                    for format in ['t','T','d','D','f','F','s','S','R'] {ui.selectable_value(&mut picker.format,format,format_name(format));}
                });
                if picker.timestamp().is_err_and(|error|error.contains("dos veces")) || picker.occurrence.is_some() {
                    ui.horizontal(|ui| {ui.selectable_value(&mut picker.occurrence,Some(false),"Primera ocurrencia");
                        ui.selectable_value(&mut picker.occurrence,Some(true),"Segunda ocurrencia");});
                }
                ui.separator();
                let value=picker.timestamp();
                match &value {Ok(stamp)=>{ui.label(display(*stamp)).on_hover_text(tooltip(*stamp));},Err(error)=>{ui.colored_label(self.theme.muted,error);}}
                if !picker.error.is_empty() {ui.colored_label(ui.visuals().error_fg_color,&picker.error);}
                ui.horizontal(|ui| {
                    if ui.add_enabled(value.is_ok(),egui::Button::new("Insertar fecha y hora")).clicked() {confirm=true;}
                    if ui.button("Cancelar").clicked() {cancel=true;}
                });
                confirm|=super::input_ownership::take_action(ctx,"timestamp-confirm") && !egui::Popup::is_any_open(ctx);
            });
        }
        if cancel {open=false;}
        if open && confirm {
            match picker.timestamp() {
                Ok(stamp)=>{
                    let Some(previous)=self.drafts.get(&picker.channel).cloned() else{return;};
                    let mut candidate=previous.clone();let mut chars=candidate.text.chars().collect::<Vec<_>>();
                    let token=format!("<t:{}:{}> ",stamp.seconds,stamp.format);chars.splice(picker.start..picker.end,token.chars());
                    candidate.text=chars.into_iter().collect();
                    super::mentions::adjust(&previous.text,&candidate.text,&mut candidate.mentions);candidate.revision=candidate.revision.saturating_add(1);
                    if self.accepts_draft_edit(&previous,&candidate,Some(&picker.channel),0) {
                        self.composer_cursor=picker.start+token.chars().count();self.drafts.insert(picker.channel.clone(),candidate);
                        let id=egui::Id::new(("composer",&picker.channel));
                        if let Some(mut edit)=egui::TextEdit::load_state(ctx,id) {
                            edit.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.composer_cursor))));edit.store(ctx,id);
                        }
                        ctx.memory_mut(|memory|memory.request_focus(id));open=false;
                    }else{picker.error=super::text_budget::ERROR.into();}
                },Err(error)=>picker.error=error,
            }
        }
        if open {self.time_picker=Some(picker);}
    }
}
fn format_name(format:char)->&'static str {
    match format {'t'=>"Hora corta",'T'=>"Hora con segundos",'d'=>"Fecha corta",'D'=>"Fecha larga",
        'f'=>"Fecha y hora",'F'=>"Fecha completa y hora",'s'=>"Fecha corta y hora",'S'=>"Fecha corta y hora con segundos",
        'R'=>"Tiempo relativo",_=>"Fecha y hora"}
}
