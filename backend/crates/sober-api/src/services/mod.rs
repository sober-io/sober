//! Service layer — business logic extracted from route handlers.

pub mod attachment;
pub mod auth;
pub mod collaborator;
pub mod conversation;
pub mod evolution;
pub mod message;
pub mod plugin;
pub mod tag;
pub mod user;
pub mod verify_membership;
pub mod ws_dispatch;

pub(crate) use verify_membership::verify_membership;
