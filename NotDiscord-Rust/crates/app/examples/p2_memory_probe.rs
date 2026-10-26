//! Opt-in allocation benchmark. Synthetic DTOs only: no account, UI, runtime or network.
//! The counting System allocator exists only in this standalone example binary.
use notdiscord::storage::MessageCache;
use notdiscord_core::{models::AppSnapshot, parsing};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

struct CountingSystem;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;
fn added(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::SeqCst) + bytes;
    PEAK.fetch_max(live, Ordering::SeqCst);
    REQUESTED.fetch_add(bytes, Ordering::SeqCst);
}
unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            added(layout.size());
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            added(layout.size());
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        DEALLOCATIONS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            added(size);
            REALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        replacement
    }
}
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Phase {
    name: &'static str,
    retained_delta_bytes: i128,
    peak_additional_bytes: usize,
    total_requested_bytes: usize,
    allocation_calls: usize,
    reallocation_calls: usize,
    deallocation_calls: usize,
    elapsed_micros: u128,
}
fn phase<T>(name: &'static str, action: impl FnOnce() -> T) -> (T, Phase) {
    let live = LIVE.load(Ordering::SeqCst);
    PEAK.store(live, Ordering::SeqCst);
    let requested = REQUESTED.load(Ordering::SeqCst);
    let allocations = ALLOCATIONS.load(Ordering::SeqCst);
    let reallocations = REALLOCATIONS.load(Ordering::SeqCst);
    let deallocations = DEALLOCATIONS.load(Ordering::SeqCst);
    let timer = Instant::now();
    let result = black_box(action());
    let elapsed_micros = timer.elapsed().as_micros();
    let retained = LIVE.load(Ordering::SeqCst) as i128 - live as i128;
    let peak = PEAK.load(Ordering::SeqCst).saturating_sub(live);
    (
        result,
        Phase {
            name,
            retained_delta_bytes: retained,
            peak_additional_bytes: peak,
            total_requested_bytes: REQUESTED.load(Ordering::SeqCst) - requested,
            allocation_calls: ALLOCATIONS.load(Ordering::SeqCst) - allocations,
            reallocation_calls: REALLOCATIONS.load(Ordering::SeqCst) - reallocations,
            deallocation_calls: DEALLOCATIONS.load(Ordering::SeqCst) - deallocations,
            elapsed_micros,
        },
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:X}", Sha256::digest(bytes))
}
fn person(index: usize) -> Value {
    json!({"Id":format!("10000000000000{index:04}"),"Name":format!("Persona sintética {index:04}"),"Username":format!("persona_{index:04}"),"DisplayName":format!("Persona sintética {index:04}"),"AvatarUrl":null})
}
fn fixture(replies: bool, members: usize, text_size: usize) -> Vec<u8> {
    let mut messages = Vec::new();
    for index in 0..200 {
        let id = format!("20000000000000{index:04}");
        let mut message = json!({"Id":id,"ChannelId":"300000000000000001","Author":person(index%20),"Content":"mensaje prueba. ".repeat(text_size/16),"Timestamp":format!("2026-10-21T{:02}:{:02}:{:02}-05:00",10+index/3600,(index/60)%60,index%60),"Attachments":[],"Mentions":[],"Reactions":[]});
        if replies && index >= 150 {
            message["ReplyTo"] = json!(format!("20000000000000{:04}", index - 150));
        }
        if index % 4 == 0 {
            message["Attachments"] = json!([{"Id":format!("40000000000000{index:04}"),"Name":"adjunto-sintetico.txt","Url":"","ContentType":"text/plain","Size":1024}]);
        }
        messages.push(message);
    }
    let roles: Vec<_> = (0..8).map(|index| json!({"Id":format!("50000000000000{index:04}"),"Name":format!("Rol sintético repetido {index}"),"Color":123456,"Position":index})).collect();
    let members: Vec<_> = (0..members).map(|index|json!({"User":person(index),"Presence":"online","Roles":roles,"Nickname":format!("Persona sintética {index:04}")})).collect();
    let dms: Vec<_> = (0..100).map(|index|json!({"Id":format!("60000000000000{index:04}"),"Name":format!("Persona sintética {index:04}"),"Kind":"dm","RecipientId":format!("10000000000000{index:04}"),"Presence":"online"})).collect();
    serde_json::to_vec(&json!({"Demo":true,"View":"chat","SignedIn":true,"CurrentUser":person(0),"SelectedChannelId":"300000000000000001","ChannelName":"Datos sintéticos","SelectedChannelKind":"text","SelectedGuildId":"700000000000000001","Messages":messages,"GuildMembers":members,"DirectMessages":dms,"ChannelAccess":{"Known":true,"CanView":true,"CanReadHistory":true,"CanSend":true,"CanViewMembers":true}})).unwrap()
}
fn scenario(name: &str, bytes: Vec<u8>) -> Value {
    let mut phases = Vec::with_capacity(7);
    let input_hash = digest(&bytes);
    let (state, measured) = phase("deserialize_snapshot", || {
        serde_json::from_slice::<AppSnapshot>(&bytes).unwrap()
    });
    phases.push(measured);
    assert!(state.demo && state.messages.len() == 200);
    let (copy, measured) = phase("clone_snapshot", || state.clone());
    phases.push(measured);
    assert_eq!(state, copy);
    drop(copy);
    let (_, measured) = phase("clone_snapshot_60_sequential_frames", || {
        for _ in 0..60 {
            black_box(state.clone());
        }
    });
    phases.push(measured);
    let input_messages = state.messages.to_vec();
    let (context, measured) = phase("context_messages_owned_input", || {
        parsing::context_messages(input_messages)
    });
    phases.push(measured);
    assert_eq!(context.len(), 200);
    assert_eq!(
        context
            .iter()
            .filter(|message| message.reply_preview.is_some())
            .count(),
        state
            .messages
            .iter()
            .filter(|message| message.reply_to.is_some())
            .count()
    );
    let context_hash =
        digest(&serde_json::to_vec(&serde_json::to_value(&context).unwrap()).unwrap());
    drop(context);
    let mut cache = MessageCache::default();
    let input_messages = state.messages.to_vec();
    let (_, measured) = phase("cache_replace_owned_input", || {
        cache.replace("300000000000000001", input_messages)
    });
    phases.push(measured);
    let (cached, measured) = phase("cache_get", || cache.get("300000000000000001"));
    phases.push(measured);
    let count = cache.count();
    assert_eq!(cached.len(), count);
    assert!(count <= 200 && cache.estimated_text_bytes() <= 4 * 1024 * 1024);
    let target = cached
        .first()
        .expect("Synthetic window must retain a message")
        .id
        .clone();
    let (found, measured) = phase("cache_find_one_message", || {
        cache.find("300000000000000001", &target)
    });
    phases.push(measured);
    assert_eq!(found.as_ref().map(|message| &message.id), Some(&target));
    drop(found);
    let canonical = serde_json::to_vec(&serde_json::to_value(&state).unwrap()).unwrap();
    let restored: AppSnapshot = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(restored, state);
    json!({"Name":name,"InputSha256":input_hash,"CanonicalSnapshotSha256":digest(&canonical),"ContextSha256":context_hash,"MessageCount":state.messages.len(),"MemberCount":state.guild_members.len(),"DmCount":state.direct_messages.len(),"CachedMessages":count,"CacheEstimatedTextBytes":cache.estimated_text_bytes(),"Phases":phases})
}
fn main() {
    let output = std::env::args()
        .nth(1)
        .expect("An explicit output JSON path is required");
    let output = std::path::PathBuf::from(output);
    let scenarios = vec![
        scenario("messages_200_no_replies_16k", fixture(false, 0, 16 * 1024)),
        scenario("messages_200_50_replies_16k", fixture(true, 0, 16 * 1024)),
        scenario(
            "snapshot_stress_2000_members_8_roles",
            fixture(true, 2000, 2048),
        ),
    ];
    let report = json!({"Method":"Requested Rust System allocator bytes/calls only, single-threaded synthetic DTO/cache workloads; input JSON generation and output serialization outside measured phases. Phases include their internal temporaries, not allocator overhead, native heap, process RAM, GPU or network. Owned inputs are prepared before context/cache phases. Stress membership is a synthetic DTO stress case, not an assertion of live view capacity.","Authenticated":false,"UiStarted":false,"MediaHostStarted":false,"NetworkUsed":false,"Scenarios":scenarios});
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
