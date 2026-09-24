//! Serial background phone-history and attachment prefetch.

use crate::model::{Chat, ChatId};
use crate::settings::HistoryPrefetch;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub const HISTORY_GAP: Duration = Duration::from_secs(20);
pub const MEDIA_GAP: Duration = Duration::from_secs(3);
pub const MEDIA_MAX: u64 = 64 * 1024 * 1024;
const RECENT_LIMIT: usize = 10;

pub(super) struct State {
    pub mode: HistoryPrefetch,
    pub focused: Option<ChatId>,
    exhausted: HashSet<ChatId>,
    history_due: Option<Instant>,
    history_failures: u32,
    history_chat: Option<ChatId>,
    media_due: Option<Instant>,
    media: Option<(ChatId, String)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: HistoryPrefetch::Off,
            focused: None,
            exhausted: HashSet::new(),
            history_due: None,
            history_failures: 0,
            history_chat: None,
            media_due: None,
            media: None,
        }
    }
}

impl State {
    pub fn configure(&mut self, mode: HistoryPrefetch, focused: Option<ChatId>) {
        self.mode = mode;
        self.focused = focused;
        if mode == HistoryPrefetch::Off {
            self.history_chat = None;
            self.media = None;
        }
    }

    /// After reconnect, phone history may be available again.
    pub fn on_connected(&mut self) {
        self.exhausted.clear();
        self.history_failures = 0;
        self.history_due = None;
        self.history_chat = None;
    }

    pub fn reset_session(&mut self) {
        let mode = self.mode;
        let focused = self.focused.clone();
        *self = Self::default();
        self.mode = mode;
        self.focused = focused;
    }

    pub fn next_history(
        &self,
        now: Instant,
        phone_busy: bool,
        targets: &[ChatId],
    ) -> Option<ChatId> {
        if self.mode == HistoryPrefetch::Off || phone_busy || self.history_chat.is_some() {
            return None;
        }
        if self.history_due.is_some_and(|due| now < due) {
            return None;
        }
        targets
            .iter()
            .find(|chat| !self.exhausted.contains(*chat))
            .cloned()
    }

    pub fn start_history(&mut self, chat: ChatId) {
        self.history_chat = Some(chat);
    }

    /// True when this completion belongs to the prefetch request.
    pub fn finish_history(&mut self, chat: &str, more: bool, now: Instant) -> bool {
        if self.history_chat.as_deref() != Some(chat) {
            return false;
        }
        self.history_chat = None;
        self.history_failures = 0;
        self.history_due = Some(now + HISTORY_GAP);
        if !more {
            self.exhausted.insert(chat.to_owned());
        }
        true
    }

    pub fn fail_history(&mut self, chat: &str, now: Instant) -> bool {
        if self.history_chat.as_deref() != Some(chat) {
            return false;
        }
        self.history_chat = None;
        self.history_failures = self.history_failures.saturating_add(1);
        let shift = (self.history_failures - 1).min(5);
        let delay = (30 * (1_u64 << shift)).min(900);
        self.history_due = Some(now + Duration::from_secs(delay));
        true
    }

    pub fn next_media_ready(&self, now: Instant) -> bool {
        self.mode != HistoryPrefetch::Off
            && self.media.is_none()
            && self.media_due.is_none_or(|due| now >= due)
    }

    pub fn skip_media(&self, chat: &str, id: &str) -> bool {
        self.media
            .as_ref()
            .is_some_and(|(active_chat, active_id)| active_chat == chat && active_id == id)
    }

    pub fn start_media(&mut self, chat: ChatId, id: String) {
        self.media = Some((chat, id));
    }

    /// True when this completion belongs to the prefetch download.
    pub fn finish_media(&mut self, chat: &str, id: &str, now: Instant) -> bool {
        if self
            .media
            .as_ref()
            .is_none_or(|(active_chat, active_id)| active_chat != chat || active_id != id)
        {
            return false;
        }
        self.media = None;
        self.media_due = Some(now + MEDIA_GAP);
        true
    }
}

/// Open chat first when it belongs in the mode, then pinned (top first), then
/// the ten most recently active unpinned chats that are not archived.
pub(super) fn targets(chats: &[Chat], mode: HistoryPrefetch, focused: Option<&str>) -> Vec<ChatId> {
    match mode {
        HistoryPrefetch::Off => Vec::new(),
        HistoryPrefetch::Focused => focused.map(|id| vec![id.to_owned()]).unwrap_or_default(),
        HistoryPrefetch::RecentAndPinned => {
            let mut out = Vec::new();
            if let Some(id) = focused
                && chats.iter().any(|chat| chat.id == id)
            {
                out.push(id.to_owned());
            }
            let mut pinned: Vec<&Chat> = chats.iter().filter(|chat| chat.pinned).collect();
            pinned.sort_by_key(|chat| std::cmp::Reverse(chat.pinned_at));
            for chat in pinned {
                push_unique(&mut out, &chat.id);
            }
            let mut recent: Vec<&Chat> = chats
                .iter()
                .filter(|chat| !chat.pinned && !chat.archived)
                .collect();
            recent.sort_by_key(|chat| std::cmp::Reverse(chat.last_activity));
            for chat in recent.into_iter().take(RECENT_LIMIT) {
                push_unique(&mut out, &chat.id);
            }
            out
        }
    }
}

fn push_unique(out: &mut Vec<ChatId>, id: &str) {
    if !out.iter().any(|known| known == id) {
        out.push(id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(id: &str, last_activity: i64, pinned: bool, pinned_at: i64, archived: bool) -> Chat {
        let mut chat = Chat::new(id.into(), id.into());
        chat.last_activity = last_activity;
        chat.pinned = pinned;
        chat.pinned_at = pinned_at;
        chat.archived = archived;
        chat
    }

    #[test]
    fn targets_put_focused_then_pinned_then_recent() {
        let chats = vec![
            chat("pin-low", 1, true, 1, false),
            chat("pin-high", 2, true, 9, false),
            chat("old", 10, false, 0, false),
            chat("focus", 50, false, 0, false),
            chat("archived", 90, false, 0, true),
            chat("r1", 40, false, 0, false),
        ];
        assert!(targets(&chats, HistoryPrefetch::Off, Some("focus")).is_empty());
        assert_eq!(
            targets(&chats, HistoryPrefetch::Focused, Some("focus")),
            vec!["focus".to_owned()]
        );
        assert_eq!(
            targets(&chats, HistoryPrefetch::RecentAndPinned, Some("focus")),
            vec![
                "focus".to_owned(),
                "pin-high".to_owned(),
                "pin-low".to_owned(),
                "r1".to_owned(),
                "old".to_owned(),
            ]
        );
    }

    #[test]
    fn recent_mode_keeps_every_pin_and_ten_unpinned() {
        let mut chats = vec![chat("pin", 1, true, 1, false)];
        for index in 0..12 {
            chats.push(chat(&format!("r{index}"), 100 - index, false, 0, false));
        }
        let ids = targets(&chats, HistoryPrefetch::RecentAndPinned, None);
        assert_eq!(ids[0], "pin");
        assert_eq!(ids.len(), 11);
        assert!(!ids.iter().any(|id| id == "r10" || id == "r11"));
    }

    #[test]
    fn history_waits_for_user_and_backs_off_on_failure() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        let now = Instant::now();
        let targets = vec!["a".into(), "b".into()];
        assert!(state.next_history(now, true, &targets).is_none());
        assert_eq!(
            state.next_history(now, false, &targets).as_deref(),
            Some("a")
        );
        state.start_history("a".into());
        assert!(state.next_history(now, false, &targets).is_none());
        assert!(state.fail_history("a", now));
        assert!(state.next_history(now, false, &targets).is_none());
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(30), false, &targets)
                .as_deref(),
            Some("a")
        );
        state.start_history("a".into());
        assert!(state.finish_history("a", false, now + Duration::from_secs(30)));
        assert!(
            state
                .next_history(now + Duration::from_secs(31), false, &targets)
                .is_none()
        );
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(51), false, &targets)
                .as_deref(),
            Some("b")
        );
    }

    #[test]
    fn a_foreign_history_ack_does_not_advance_the_queue() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        state.start_history("a".into());
        assert!(!state.finish_history("other", true, Instant::now()));
        assert!(
            state
                .next_history(Instant::now(), false, &["a".into()])
                .is_none()
        );
    }

    #[test]
    fn a_failed_prefetch_file_is_not_skipped_forever() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        let now = Instant::now();
        state.start_media("a".into(), "m1".into());
        assert!(state.skip_media("a", "m1"));
        assert!(state.finish_media("a", "m1", now));
        assert!(!state.skip_media("a", "m1"));
        assert!(!state.next_media_ready(now));
        assert!(state.next_media_ready(now + MEDIA_GAP));
        assert!(!state.finish_media("a", "m1", now + MEDIA_GAP));
    }
}
