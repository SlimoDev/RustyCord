//! Shared mention policy. This module does not resolve users or perform I/O.
use crate::models::ChannelAccess;
use serde_json::{Value, json};

pub fn everyone_allowed(guild_id: &str, access: &ChannelAccess) -> bool {
    !guild_id.is_empty()
        && access.known
        && access.can_view
        && access.can_send
        && access.can_mention_everyone
}

/// Whether this exact byte range is a broadcast token outside escaped/code text.
/// The service's `mention_everyone` metadata remains authoritative for received
/// messages; finding a literal token alone never proves that a ping occurred.
pub fn broadcast_at(text: &str, start: usize, end: usize) -> bool {
    if !matches!(text.get(start..end), Some("@everyone" | "@here")) {
        return false;
    }
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '@')
        || text[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return false;
    }
    plain_text_at(text, start)
}

/// Shared lexical guard for user/broadcast mention renderers and editors.
pub fn plain_text_at(text: &str, at: usize) -> bool {
    if at > text.len() || !text.is_char_boundary(at) {
        return false;
    }
    let bytes = text.as_bytes();
    let escaped = |at: usize| {
        bytes[..at]
            .iter()
            .rev()
            .take_while(|b| **b == b'\\')
            .count()
            % 2
            == 1
    };
    if escaped(at) {
        return false;
    }
    let mut delimiter = 0;
    let mut cursor = 0;
    while cursor < at {
        if bytes[cursor] == b'`' && !escaped(cursor) {
            let run = bytes[cursor..at].iter().take_while(|b| **b == b'`').count();
            if delimiter == 0 {
                delimiter = run;
            } else if run == delimiter {
                delimiter = 0;
            }
            cursor += run;
        } else {
            cursor += 1;
        }
    }
    delimiter == 0
}

pub fn has_broadcast(text: &str) -> bool {
    text.match_indices('@').any(|(start, _)| {
        ["@everyone", "@here"].iter().any(|token| {
            text[start..].starts_with(token) && broadcast_at(text, start, start + token.len())
        })
    })
}

/// All send paths (plain, edit and multipart) use the same explicit policy.
/// Never silently enable reply pings or bypass the effective channel ACL.
pub fn allowed_mentions(content: &str, may_broadcast: bool) -> Value {
    let parse = if may_broadcast && has_broadcast(content) {
        vec!["users", "roles", "everyone"]
    } else {
        vec!["users", "roles"]
    };
    json!({"parse": parse, "replied_user": false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broadcast_literals_need_context_permissions_and_non_code_content() {
        let mut access = ChannelAccess::private();
        access.can_mention_everyone = true;
        assert!(!everyone_allowed("", &access));
        assert!(everyone_allowed("10", &access));
        access.known = false;
        assert!(!everyone_allowed("10", &access));
        access.known = true;
        access.can_send = false;
        assert!(!everyone_allowed("10", &access));
        for content in [
            "hello@everyone",
            "@everyone_else",
            r"\@everyone",
            "`@everyone`",
            "```rust\n@everyone\n```",
            "ordinary text",
        ] {
            assert!(!has_broadcast(content), "{content}");
            assert_eq!(
                allowed_mentions(content, true)["parse"],
                json!(["users", "roles"])
            );
        }
        for content in [
            "@everyone",
            "Hello (@everyone)!",
            "@here online",
            "`code` @everyone",
            "é @everyone",
        ] {
            assert!(has_broadcast(content), "{content}");
            assert_eq!(
                allowed_mentions(content, true)["parse"],
                json!(["users", "roles", "everyone"])
            );
            assert_eq!(
                allowed_mentions(content, false)["parse"],
                json!(["users", "roles"])
            );
        }
    }
    #[test]
    fn range_validation_never_panics_or_treats_other_mentions_as_broadcasts() {
        for (start, end) in [(0, 100), (1, 2), (2, 1), (100, 101)] {
            assert!(!broadcast_at("é@everyone", start, end));
        }
        assert!(!has_broadcast("<@123> <@&456> @EveryOne"));
        assert_eq!(allowed_mentions("@everyone", true)["replied_user"], false);
    }
}
