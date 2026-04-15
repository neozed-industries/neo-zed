use std::cmp::Ordering;

use super::*;

/// Stores two notions of time:
/// - last accessed - the last time the user clicked on a thread to open it in
///   the agent panel (or used a keyboard shortcut). This is used for ordering the
///   ctrl-tab switcher
/// - last message sent or queued. This is used to order the threads in the
///   sidebar, and also as a secondary sort in the ctrl-tab switcher
///
/// Both times are "stable" w.r.t. user interaction - that is, a specific
/// thread's time cannot change without user interaction, so we can use them to
/// order UI elements.
///
/// [`ThreadMetadata::updated_at`] can be used as a sensible fallback for both.
#[derive(Default)]
pub(super) struct ThreadInteractionTimes {
    last_accessed: HashMap<ThreadId, DateTime<Utc>>,
    last_message_sent_or_queued: HashMap<ThreadId, DateTime<Utc>>,
}

impl ThreadInteractionTimes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Should be called whenever a thread is opened/focused in the agent panel
    pub fn record_access(&mut self, id: ThreadId) {
        self.last_accessed.insert(id, Utc::now());
    }

    /// Should be called any time a thread has a message sent or queued (but not
    /// when a queued message is finally sent).
    pub fn record_message_sent_or_queued(&mut self, id: ThreadId) {
        self.last_message_sent_or_queued.insert(id, Utc::now());
    }

    pub fn retain(&mut self, f: impl Fn(&ThreadId) -> bool) {
        self.last_accessed.retain(|id, _| f(id));
        self.last_message_sent_or_queued.retain(|id, _| f(id));
    }

    pub fn last_accessed(&self, thread: &ThreadMetadata) -> DateTime<Utc> {
        self.last_accessed
            .get(&thread.thread_id)
            .cloned()
            .unwrap_or_else(|| thread.updated_at)
    }

    pub fn last_message_sent_or_queued(&self, thread: &ThreadMetadata) -> DateTime<Utc> {
        self.last_message_sent_or_queued
            .get(&thread.thread_id)
            .cloned()
            .unwrap_or_else(|| thread.updated_at)
    }

    /// How threads should be sorted in the ctrl-tab switcher
    pub fn cmp_for_tab_switcher(&self, lhs: &ThreadMetadata, rhs: &ThreadMetadata) -> Ordering {
        let lhs = self.last_accessed(lhs);
        let rhs = self.last_accessed(rhs);

        lhs.cmp(&rhs).reverse()
    }

    /// How threads should be sorted in the sidebar
    pub fn cmp_for_sidebar(&self, lhs: &ThreadMetadata, rhs: &ThreadMetadata) -> Ordering {
        let lhs = self.last_message_sent_or_queued(lhs);
        let rhs = self.last_message_sent_or_queued(rhs);

        lhs.cmp(&rhs).reverse()
    }
}
