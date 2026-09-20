//! Headless core of the Pakpos HTTP client.
//!
//! This crate contains the domain model, application state machine, protocol
//! adapters, HTTP execution, response processing, and SQLite persistence. GTK
//! composition and worker orchestration live in the binary crate so these
//! modules remain usable by headless tests.
//!
//! The main dependency rule is that [`app`] may describe external work through
//! effects but may not perform that work. Callers execute those effects through
//! the binary runtime and return typed results to the application state.

pub mod app;
pub mod collections;
pub mod curl;
pub mod models;
pub mod net;
pub mod postman;
pub mod response;
pub mod storage;
