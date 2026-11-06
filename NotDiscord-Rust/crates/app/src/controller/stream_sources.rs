//! Publication authority is independent from a viewer's decoder/socket.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase { Active, Recovering, Unavailable, Ended }
#[derive(Clone, Debug)]
struct Publication { revision:u64, identity:String, phase:Phase }
#[derive(Default)]
pub(super) struct Sources { next:u64, entries:HashMap<String,Publication> }
impl Sources {
    /// Only ordered publisher metadata can open a new publication. Native
    /// callbacks and a watch ACK must never call this method as authority.
    pub(super) fn announce(&mut self,key:&str,identity:&str,new_publication:bool)->Option<u64> {
        if let Some(old)=self.entries.get_mut(key) {
            let changed=!identity.is_empty()&&!old.identity.is_empty()&&old.identity!=identity;
            if old.identity.is_empty()&&!identity.is_empty(){old.identity=identity.into();}
            if !changed && !(old.phase==Phase::Ended&&new_publication) {return Some(old.revision);}
        } else if self.entries.len()>=256 {return None;}
        self.next=self.next.wrapping_add(1).max(1);
        self.entries.insert(key.into(),Publication {revision:self.next,identity:identity.into(),phase:Phase::Active});
        Some(self.next)
    }
    pub(super) fn revision(&self,key:&str)->Option<u64> {self.entries.get(key).map(|p|p.revision)}
    pub(super) fn phase(&self,key:&str)->Option<Phase> {self.entries.get(key).map(|p|p.phase)}
    pub(super) fn blocked(&self,key:&str)->bool {matches!(self.phase(key),Some(Phase::Ended|Phase::Unavailable))||self.phase(key).is_none()&&self.entries.len()>=256}
    pub(super) fn recover(&mut self,key:&str)->Option<u64> {
        let p=self.entries.get_mut(key)?;
        if p.phase!=Phase::Active {return None;}
        p.phase=Phase::Recovering;Some(p.revision)
    }
    pub(super) fn recovered(&mut self,key:&str,revision:u64)->bool {
        let Some(p)=self.entries.get_mut(key).filter(|p|p.revision==revision&&p.phase==Phase::Recovering) else {return false;};
        p.phase=Phase::Active;true
    }
    pub(super) fn unavailable(&mut self,key:&str,revision:u64)->bool {
        let Some(p)=self.entries.get_mut(key).filter(|p|p.revision==revision&&p.phase!=Phase::Ended) else {return false;};
        p.phase=Phase::Unavailable;true
    }
    /// Returns the semantic transition exactly once, including when a decoder
    /// was never opened or an ambiguous subscription failed earlier.
    pub(super) fn end(&mut self,key:&str)->Option<u64> {
        let p=self.entries.get_mut(key)?;
        if p.phase==Phase::Ended {return None;}
        p.phase=Phase::Ended;Some(p.revision)
    }
    pub(super) fn clear(&mut self) {self.entries.clear();}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriber_failure_and_late_callbacks_do_not_end_or_reopen_a_publication() {
        let mut s=Sources::default();let first=s.announce("screen","rtc1",false).unwrap();
        assert_eq!(s.recover("screen"),Some(first));assert_eq!(s.recover("screen"),None);
        assert!(s.unavailable("screen",first));
        assert_eq!(s.announce("screen","rtc1",false),Some(first));assert!(s.blocked("screen"));
        assert_eq!(s.end("screen"),Some(first));assert_eq!(s.end("screen"),None);
        assert!(!s.recovered("screen",first));
    }
    #[test]
    fn a_reused_key_has_a_new_revision_and_old_completion_has_no_authority() {
        let mut s=Sources::default();let first=s.announce("screen","rtc1",false).unwrap();
        s.end("screen");let second=s.announce("screen","rtc1",true).unwrap();
        assert_ne!(first,second);assert!(!s.unavailable("screen",first));
        assert_eq!(s.phase("screen"),Some(Phase::Active));
        let third=s.announce("screen","rtc2",false).unwrap();assert_ne!(second,third);
    }
    #[test]
    fn identity_and_tombstones_are_bounded_without_rearming_by_eviction() {
        let mut s=Sources::default();
        for n in 0..256 {let key=n.to_string();s.announce(&key,"rtc",false).unwrap();s.end(&key);}
        assert_eq!(s.announce("overflow","rtc",false),None);
        assert_eq!(s.end("0"),None);s.clear();assert!(s.announce("overflow","rtc",false).is_some());
    }
    #[test]
    fn the_first_rtc_identity_enriches_the_offer_and_a_later_identity_starts_a_new_publication() {
        let mut s=Sources::default();let first=s.announce("screen","",false).unwrap();
        assert_eq!(s.announce("screen","rtc1",false),Some(first));
        s.unavailable("screen",first);
        assert_eq!(s.announce("screen","rtc1",true),Some(first));assert!(s.blocked("screen"));
        let next=s.announce("screen","rtc2",false).unwrap();assert_ne!(first,next);assert!(!s.blocked("screen"));
    }
}
