//! Fixed vocabulary and bounded causal data. Never contains payloads, remote
//! close text, URLs, user IDs or credentials; counters are local and ephemeral.
use super::*;
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayExitReason {
    Unknown, ConnectFailed, ConnectTimeout, Authentication, LocalCancelled,
    SafetyStopped, PowerChanged, HelloTimeout, ReadyTimeout, HeartbeatTimeout,
    WriterStopped, ReaderFailed, Eof, RemoteClose, ServerReconnect, InvalidSession,
    ProtocolFrame, ProtocolHandshake, DeliveryDeadline, DeliverySaturated, ControlSaturated,
}
impl GatewayExitReason {
    pub fn label(self)->&'static str {
        match self {
            Self::HeartbeatTimeout=>"No llegó la confirmación de latido del Gateway.",
            Self::DeliveryDeadline|Self::DeliverySaturated=>"La entrega local del Gateway no pudo continuar.",
            Self::WriterStopped|Self::ControlSaturated=>"La escritura local del Gateway se detuvo.",
            Self::PowerChanged=>"El Gateway se pausó por un cambio de energía.",
            Self::ServerReconnect|Self::InvalidSession=>"Discord solicitó renovar la conexión principal.",
            Self::HelloTimeout|Self::ReadyTimeout=>"No se completó la preparación del Gateway.",
            Self::ProtocolFrame|Self::ProtocolHandshake=>"El Gateway recibió una secuencia no compatible.",
            _=>"La conexión principal se interrumpió.",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayExit {
    pub reason:GatewayExitReason, pub close_code:Option<u16>,
    pub connection:u32, pub episode:u32, pub elapsed_ms:u32,
    pub last_ack_age_ms:Option<u32>, pub pending_events:u8, pub pending_bytes:u32,
}
impl GatewayExit {
    pub(super) fn new(reason:GatewayExitReason,close_code:Option<u16>,connection:u32,episode:u64,
        started:Instant,last_ack:Option<Instant>,pending:usize,bytes:usize)->Self {
        let now=Instant::now();
        Self {reason,close_code,connection,episode:episode.min(u32::MAX as u64) as u32,
            elapsed_ms:now.saturating_duration_since(started).as_millis().min(u32::MAX as u128) as u32,
            last_ack_age_ms:last_ack.map(|at|now.saturating_duration_since(at).as_millis().min((u32::MAX-1) as u128) as u32),
            pending_events:pending.min(32) as u8,pending_bytes:bytes.min(4*1024*1024) as u32}
    }
    pub fn cause_bits(self)->u64 {self.reason as u64|((self.close_code.unwrap_or(0) as u64)<<8)
        |((self.pending_events as u64)<<24)|((self.pending_bytes as u64)<<32)}
    pub fn timing_bits(self)->u64 {self.elapsed_ms as u64|((self.last_ack_age_ms.unwrap_or(u32::MAX) as u64)<<32)}
    pub fn owner_bits(self)->u64 {self.connection as u64|((self.episode as u64)<<32)}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn causal_summary_distinguishes_absent_ack_and_bounds_queue_data_without_free_text() {
        let now=Instant::now();
        let first=GatewayExit::new(GatewayExitReason::DeliveryDeadline,None,7,9,now,None,10_000,usize::MAX);
        assert_eq!(first.pending_events,32);assert_eq!(first.pending_bytes,4*1024*1024);
        assert_eq!(first.timing_bits()>>32,u32::MAX as u64);assert_eq!(first.owner_bits(),7|(9u64<<32));
        let ack=GatewayExit::new(GatewayExitReason::HeartbeatTimeout,Some(4009),8,9,now,Some(now),0,0);
        assert!(ack.last_ack_age_ms.is_some());assert_ne!(first.cause_bits(),ack.cause_bits());
        assert!(!format!("{ack:?}").contains("token"));
    }
}
