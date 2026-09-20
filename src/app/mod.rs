//! GTK-independent application policy.
//!
//! This layer coordinates domain state without knowing how GTK, HTTP, SQLite,
//! threads, or files are implemented. [`state::AppState`] reduces typed actions,
//! [`effect::Effect`] describes work for the runtime, and
//! [`collections::CollectionSession`] tracks the editable collection aggregate.
//! Keeping these decisions here makes navigation, dirty tracking, serialization,
//! and stale-result handling testable without a display server.

pub mod collections;
pub mod effect;
pub mod state;

pub use collections::{CollectionChanges, CollectionSession, RemoveRequestResult, RequestListItem};
pub use effect::{CollectionList, Effect, EffectOutput, PostmanImport};
pub use state::{Action, AppEvent, AppState, DeferredAction, Update};
