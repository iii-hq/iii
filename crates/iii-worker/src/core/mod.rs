// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Pure async ops backing `iii worker <cmd>` and the `worker::*` SDK
//! triggers. No CLI presentation, no `i32` exit codes, no global state.
//! Every op takes an explicit `ProjectCtx`.

pub mod error;
pub use error::{WorkerOpError, WorkerOpErrorKind};

pub mod project;
pub use project::{ProjectCtx, ProjectOperationLock};

pub mod types;
pub use types::*;

pub mod events;
pub use events::{CapturingSink, EventSink, NullSink, WorkerOpEvent};

pub mod add;

pub mod remove;

pub mod update;

pub mod start;

pub mod stop;

pub mod list;

pub mod clear;

pub mod host;
pub use host::WorkerHostShim;
