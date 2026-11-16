//! Progress reporting and cancelling, shared by the long jobs.

use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The stages a front end shows. One job reports a subset of them, mostly in this order (the
/// write sends `Settings` after `WaitDisk`, once the serial number is known).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Download,
    Check,
    Unpack,
    Settings,
    Boot,
    WaitDisk,
    /// Started, its memory found: waits for the permission to write and, in a batch, its turn.
    Ready,
    Write,
    Verify,
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A new stage started.
    Phase { phase: Phase },
    /// Bytes done out of the total within a stage.
    Progress { phase: Phase, done: u64, total: u64 },
    /// A line for the log view (rpiboot output, what was found).
    Log { line: String },
}

/// Where a job sends its events. A plain closure, so the app can turn them into webview events
/// and the CLI into lines on stderr.
pub type Sink<'a> = &'a (dyn Fn(Event) + Send + Sync);

/// A job's stop button, checked between chunks.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// An error to return from a loop that saw the stop button.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        Ok(())
    }
}

/// Reports progress at most every `step` bytes, so a 4 GB write sends hundreds of events, not
/// millions.
pub(crate) struct Throttle {
    phase: Phase,
    total: u64,
    step: u64,
    next: u64,
}

impl Throttle {
    pub(crate) fn new(phase: Phase, total: u64) -> Self {
        Self { phase, total, step: (total / 200).max(1 << 20), next: 0 }
    }

    pub(crate) fn report(&mut self, events: Sink<'_>, done: u64) {
        if done >= self.next || done >= self.total {
            events(Event::Progress { phase: self.phase, done, total: self.total });
            self.next = done.saturating_add(self.step);
        }
    }
}
