// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Queue transport adapters.
//!
//! # Legacy queue migration on the namespace upgrade
//!
//! Durable subscriptions are keyed by `(namespace, function_id)`. Subscriber
//! queues created before this change carried no namespace, so their names
//! changed and any messages left under the old names are orphaned.
//!
//! - **builtin**: auto-migrated. At adapter construction (before any consumer
//!   subscribes) [`builtin`] drains each pre-upgrade `{topic}::{fid}` queue into
//!   `{topic}::{fid}@default`. This is safe because the builtin store is
//!   single-process and fully enumerable, and idempotent by job id. A legacy
//!   message has no origin namespace, so it is migrated into `default` — the
//!   namespace everything ran under before the change.
//! - **rabbitmq**: NOT auto-migrated. Old queues (`iii.{topic}.{fid}.queue`) are
//!   left in place; their messages are orphaned. Auto-draining is unsafe on a
//!   shared broker (a rolling deploy can leave a pre-upgrade instance still
//!   consuming/producing the old queue → duplication or loss), and an orphaned
//!   message carries no origin namespace, so a drain has no correct destination
//!   (re-fanning to every namespace would duplicate work). A future
//!   operator-gated one-shot tool can drain them once the destination-namespace
//!   question is answered per deployment.
//! - **redis** / **bridge**: N/A. Redis is pub/sub (non-durable — nothing is
//!   retained), and the bridge holds no durable state (it delegates to a remote
//!   engine, whose own adapter owns migration).

pub mod bridge;
pub mod builtin;
#[cfg(feature = "test-adapters")]
pub mod memory_adapter;
#[cfg(feature = "rabbitmq")]
pub mod rabbitmq;
pub mod redis_adapter;
