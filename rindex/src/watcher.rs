use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;

/// File watcher that monitors a project directory for changes
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    event_rx: mpsc::Receiver<notify::Result<Event>>,
}

impl FileWatcher {
    pub fn new(root: &Path) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(event_tx, notify::Config::default())?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _watcher: watcher, event_rx })
    }

    /// Blocking receive of the next file system event
    pub fn next_event(&self) -> Option<Event> {
        match self.event_rx.recv() {
            Ok(Ok(event)) => Some(event),
            _ => None,
        }
    }

    /// Try to receive an event without blocking
    pub fn try_event(&self) -> Option<Event> {
        match self.event_rx.try_recv() {
            Ok(Ok(event)) => Some(event),
            _ => None,
        }
    }
}
