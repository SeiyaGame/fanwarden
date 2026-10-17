//! In-memory journal of recent log events, shown in the web UI.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

const CAPACITY: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub id: u64,
    pub ts: u64,
    pub level: &'static str,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct EventLog {
    inner: Arc<Mutex<(u64, VecDeque<LogEvent>)>>,
}

impl EventLog {
    pub fn push(&self, level: &'static str, message: String) {
        let mut guard = self.inner.lock().unwrap();
        let (next_id, events) = &mut *guard;
        *next_id += 1;
        if events.len() == CAPACITY {
            events.pop_front();
        }
        events.push_back(LogEvent { id: *next_id, ts: crate::now(), level, message });
    }

    /// Events newer than `after`, oldest first.
    pub fn since(&self, after: u64) -> Vec<LogEvent> {
        let guard = self.inner.lock().unwrap();
        guard.1.iter().filter(|e| e.id > after).cloned().collect()
    }
}

/// Tracing layer copying INFO and above into an [`EventLog`].
pub struct EventLayer(pub EventLog);

impl<S: Subscriber> Layer<S> for EventLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        if level > Level::INFO || !event.metadata().target().starts_with("fanwarden") {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let level = match level {
            Level::ERROR => "error",
            Level::WARN => "warn",
            _ => "info",
        };
        self.0.push(level, visitor.0);
    }
}

#[derive(Default)]
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }
}
