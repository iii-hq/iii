// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `EventSink` impl that renders progress events as colored stderr
//! lines, matching the existing CLI output style.

use crate::core::{EventSink, WorkerOpEvent};
use colored::Colorize;

pub struct StderrSink {
    pub brief: bool,
}

impl StderrSink {
    pub fn new(brief: bool) -> Self {
        Self { brief }
    }
}

impl EventSink for StderrSink {
    fn emit(&self, event: WorkerOpEvent) {
        // Minimal sink: the `handle_managed_*` bodies still print their
        // own colored progress, so we only react to the optional events.
        if self.brief {
            return;
        }
        match event {
            WorkerOpEvent::Started { op: _, worker: _ }
            | WorkerOpEvent::Done { op: _, worker: _ } => {}
            WorkerOpEvent::Stage {
                op: _,
                stage,
                worker,
            } => {
                eprintln!("  {} {} {}", "•".cyan(), stage, worker.bold());
            }
            WorkerOpEvent::PullProgress { worker, fraction } => {
                eprintln!("  ↓ {} {:.0}%", worker.bold(), fraction * 100.0);
            }
        }
    }
}
