//! Discord account transport and pure compatibility models for the native Rust client.
//! Network operations are explicit; this crate never scans other clients' storage.
pub mod access;
pub mod api;
mod api_operations;
pub mod catalog;
pub mod diagnostics;
pub mod gateway;
pub mod game_presence;
pub mod guild_settings;
pub mod models;
pub mod mentions;
pub mod member_moderation;
pub mod markdown;
pub mod notifications;
pub mod parsing;
mod rate_limits;
pub mod safety;
pub mod search;
pub mod settings_proto;
pub mod video;
pub mod uploads;

pub use api::{Api, ServiceError};
pub use models::*;
