// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::sync::Mutex;

/// Progress events emitted during long-running ops. CLI binds this to
/// stderr; the daemon binds this to `tracing::info!` events.
#[derive(Debug, Clone)]
pub enum WorkerOpEvent {
    Started {
        op: &'static str,
        worker: String,
    },
    PullProgress {
        worker: String,
        fraction: f64,
    },
    Stage {
        op: &'static str,
        stage: &'static str,
        worker: String,
    },
    Done {
        op: &'static str,
        worker: String,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: WorkerOpEvent);
}

/// Drops every event. Used in tests and where progress doesn't matter.
pub struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _event: WorkerOpEvent) {}
}

/// Captures every event in memory. Tests assert against this.
pub struct CapturingSink {
    pub events: Mutex<Vec<WorkerOpEvent>>,
}
impl CapturingSink {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}
impl Default for CapturingSink {
    fn default() -> Self {
        Self::new()
    }
}
impl EventSink for CapturingSink {
    fn emit(&self, event: WorkerOpEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sink_swallows_events_silently() {
        let sink = NullSink;
        sink.emit(WorkerOpEvent::Started {
            op: "add",
            worker: "x".into(),
        });
        // No panic, no assertion needed beyond compilation.
    }

    #[test]
    fn capturing_sink_records_emitted_events_in_order() {
        let sink = CapturingSink::new();
        sink.emit(WorkerOpEvent::Started {
            op: "add",
            worker: "a".into(),
        });
        sink.emit(WorkerOpEvent::Stage {
            op: "add",
            stage: "downloading",
            worker: "a".into(),
        });
        sink.emit(WorkerOpEvent::Done {
            op: "add",
            worker: "a".into(),
        });
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert!(
            matches!(&events[0], WorkerOpEvent::Started { op: "add", worker } if worker == "a")
        );
        assert!(matches!(
            &events[1],
            WorkerOpEvent::Stage {
                op: "add",
                stage: "downloading",
                ..
            }
        ));
        assert!(matches!(&events[2], WorkerOpEvent::Done { op: "add", .. }));
    }
}
