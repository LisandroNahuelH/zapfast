//! Serial background phone-history prefetch.

use crate::model::{Chat, ChatId};
use crate::settings::HistoryPrefetch;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

pub const HISTORY_GAP: Duration = Duration::from_secs(20);
/// Pages one chat may be asked for in a session. Without it a chat with a long
/// history would be drained in one run, and the phone asked for page after
/// page until it said it had nothing more.
pub const HISTORY_MAX_PAGES: u32 = 5;
const RECENT_LIMIT: usize = 10;

pub(super) struct State {
    pub mode: HistoryPrefetch,
    pub focused: Option<ChatId>,
    /// The chats still to ask, in the order they are asked. It is built from
    /// the chat list when that changes and kept in memory in between: the tick
    /// reads no archive and scans no chat.
    queue: VecDeque<ChatId>,
    /// Pages asked per chat in this session, which is what the cap counts.
    pages: HashMap<ChatId, u32>,
    /// Chats the phone said it has no more of, until the link reconnects.
    exhausted: HashSet<ChatId>,
    /// The chat list changed, or the mode did: the queue has to be built
    /// again, and only then.
    rebuild: bool,
    history_due: Option<Instant>,
    history_failures: u32,
    history_chat: Option<ChatId>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: HistoryPrefetch::Off,
            focused: None,
            queue: VecDeque::new(),
            pages: HashMap::new(),
            exhausted: HashSet::new(),
            rebuild: false,
            history_due: None,
            history_failures: 0,
            history_chat: None,
        }
    }
}

impl State {
    pub fn configure(&mut self, mode: HistoryPrefetch, focused: Option<ChatId>) {
        if mode != self.mode {
            // Another set of chats is wanted, so the queue is built again.
            self.rebuild = true;
        }
        self.mode = mode;
        self.focused = focused;
        if mode == HistoryPrefetch::Off {
            self.queue.clear();
            self.history_chat = None;
        }
        self.promote_focused();
    }

    /// After reconnect, phone history may be available again.
    pub fn on_connected(&mut self) {
        self.exhausted.clear();
        self.history_failures = 0;
        self.history_due = None;
        self.history_chat = None;
        self.rebuild = true;
    }

    /// A new link is a new session: the pages a chat was asked for are not
    /// carried over, so every target starts again from the top.
    pub fn reset_session(&mut self) {
        let mode = self.mode;
        let focused = self.focused.clone();
        *self = Self::default();
        self.mode = mode;
        self.focused = focused;
        self.rebuild = mode != HistoryPrefetch::Off;
    }

    /// Whether the queue has to be built from the chat list before the next
    /// request. Only `true` after a mode change, a reconnect or a relink.
    pub fn needs_chats(&self) -> bool {
        self.rebuild
    }

    /// Rebuilds the queue from the chat list: chats that are gone, that the
    /// phone has no more of, or that reached the cap leave, and the ones that
    /// are new join at the back. The order the rest already had is kept, so a
    /// chat that rotated away does not jump back to the front because a
    /// message arrived.
    pub fn set_chats(&mut self, chats: &[Chat]) {
        let wanted = targets(chats, self.mode, self.focused.as_deref());
        let exhausted = &self.exhausted;
        let pages = &self.pages;
        let askable = |chat: &ChatId| {
            !exhausted.contains(chat) && pages.get(chat).copied().unwrap_or(0) < HISTORY_MAX_PAGES
        };
        self.queue
            .retain(|chat| wanted.contains(chat) && askable(chat));
        for chat in wanted {
            if !self.queue.contains(&chat) && askable(&chat) {
                self.queue.push_back(chat);
            }
        }
        self.rebuild = false;
    }

    /// Whether this chat still has a turn in this session.
    fn may_ask(&self, chat: &ChatId) -> bool {
        !self.exhausted.contains(chat)
            && self.pages.get(chat).copied().unwrap_or(0) < HISTORY_MAX_PAGES
    }

    /// The open chat is asked first, wherever it is in the queue.
    fn promote_focused(&mut self) {
        let Some(focused) = self.focused.clone() else {
            return;
        };
        if let Some(position) = self.queue.iter().position(|chat| *chat == focused) {
            self.queue.remove(position);
            self.queue.push_front(focused);
        }
    }

    pub fn next_history(&self, now: Instant, phone_busy: bool) -> Option<ChatId> {
        if self.mode == HistoryPrefetch::Off || phone_busy || self.history_chat.is_some() {
            return None;
        }
        if self.history_due.is_some_and(|due| now < due) {
            return None;
        }
        self.queue.front().cloned()
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
        self.count_page(chat);
        if more {
            // The phone has more, but the other chats get their turn first.
            self.rotate(chat);
        } else {
            self.exhausted.insert(chat.to_owned());
            self.leave(chat);
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
        // A chat the phone does not answer is a turn spent like any other: it
        // goes to the back, so it stops holding up the chats behind it.
        self.count_page(chat);
        self.rotate(chat);
        true
    }

    /// Forgets a chat that is gone or emptied. It holds neither the history
    /// turn nor a place in the queue, so a request for it that can never be
    /// answered does not keep the others waiting until the next reconnect.
    pub fn forget_chat(&mut self, chat: &str) {
        if self.history_chat.as_deref() == Some(chat) {
            self.history_chat = None;
        }
        self.queue.retain(|queued| queued != chat);
        self.pages.remove(chat);
        self.exhausted.remove(chat);
    }

    fn count_page(&mut self, chat: &str) {
        let pages = self.pages.entry(chat.to_owned()).or_insert(0);
        *pages = pages.saturating_add(1);
    }

    /// Moves a chat to the back of the queue, or drops it once it has had the
    /// pages it is allowed in this session.
    fn rotate(&mut self, chat: &str) {
        self.leave(chat);
        if self.may_ask(&chat.to_owned()) {
            self.queue.push_back(chat.to_owned());
        }
    }

    fn leave(&mut self, chat: &str) {
        self.queue.retain(|queued| queued != chat);
    }
}

/// Open chat first when it belongs in the mode, then pinned (top first), then
/// the ten most recently active unpinned chats that are not archived.
pub(super) fn targets(chats: &[Chat], mode: HistoryPrefetch, focused: Option<&str>) -> Vec<ChatId> {
    match mode {
        HistoryPrefetch::Off => Vec::new(),
        HistoryPrefetch::Focused => {
            // The chat that was open can be gone: it was deleted while it was
            // focused, and the mode must not keep asking for it.
            let mut out = Vec::new();
            if let Some(id) = focused
                && chats.iter().any(|chat| chat.id == id)
            {
                out.push(id.to_owned());
            }
            out
        }
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

    /// A chat is asked for the pages it is allowed and no more, and it leaves
    /// the queue for the rest of the session.
    #[test]
    fn a_chat_leaves_the_queue_at_the_page_cap() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        state.set_chats(&[chat("a", 1, false, 0, false)]);
        // Each page waits out the gap before the next one is asked for.
        let mut now = Instant::now();
        let mut asked = 0;
        while let Some(chat) = state.next_history(now, false) {
            state.start_history(chat.clone());
            assert!(state.finish_history(&chat, true, now));
            asked += 1;
            assert!(asked <= HISTORY_MAX_PAGES + 1, "the cap has to hold");
            now += HISTORY_GAP;
        }
        assert_eq!(asked, HISTORY_MAX_PAGES);
        assert!(
            state.next_history(now, false).is_none(),
            "the chat is done for the session"
        );
    }

    /// The phone never answers a chat that is gone or offline. That chat goes
    /// to the back, so the ones behind it get their turn.
    #[test]
    fn a_failing_chat_goes_to_the_back_of_the_queue() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::RecentAndPinned, None);
        state.set_chats(&[
            chat("a", 30, false, 0, false),
            chat("b", 20, false, 0, false),
            chat("c", 10, false, 0, false),
        ]);
        let now = Instant::now();
        assert_eq!(state.next_history(now, false).as_deref(), Some("a"));
        state.start_history("a".into());
        assert!(state.fail_history("a", now));
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(30), false)
                .as_deref(),
            Some("b"),
            "the chat that did not answer does not hold the head"
        );
        state.start_history("b".into());
        assert!(state.fail_history("b", now + Duration::from_secs(30)));
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(90), false)
                .as_deref(),
            Some("c"),
            "the second failure backs off longer, and the chat behind it is not held up"
        );
        state.start_history("c".into());
        assert!(state.finish_history("c", true, now + Duration::from_secs(90)));
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(110), false)
                .as_deref(),
            Some("a"),
            "a chat comes back after its backoff, behind the others"
        );
    }

    #[test]
    fn a_chat_the_phone_has_no_more_of_leaves_the_queue() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::RecentAndPinned, None);
        state.set_chats(&[
            chat("a", 30, false, 0, false),
            chat("b", 20, false, 0, false),
        ]);
        let now = Instant::now();
        state.start_history("a".into());
        assert!(state.finish_history("a", false, now));
        assert_eq!(
            state.next_history(now + HISTORY_GAP, false).as_deref(),
            Some("b")
        );
        state.start_history("b".into());
        assert!(state.finish_history("b", false, now + HISTORY_GAP));
        assert!(
            state
                .next_history(now + Duration::from_secs(60), false)
                .is_none(),
            "nothing is left to ask"
        );
        // A reconnect is when the phone may have history for us again.
        state.on_connected();
        state.set_chats(&[
            chat("a", 30, false, 0, false),
            chat("b", 20, false, 0, false),
        ]);
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(60), false)
                .as_deref(),
            Some("a")
        );
    }

    /// The queue is built from the chat list, and a new chat list does not
    /// undo the rotation: chats already asked stay behind the ones that were
    /// not, and a chat that is gone leaves.
    #[test]
    fn the_chat_list_refreshes_the_queue_without_undoing_the_rotation() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::RecentAndPinned, None);
        state.set_chats(&[
            chat("a", 30, false, 0, false),
            chat("b", 20, false, 0, false),
        ]);
        assert!(!state.needs_chats(), "the list has just been read");
        let now = Instant::now();
        state.start_history("a".into());
        assert!(state.finish_history("a", true, now));
        // A message arrives in a new chat: the list is read again.
        state.set_chats(&[
            chat("a", 30, false, 0, false),
            chat("b", 20, false, 0, false),
            chat("new", 40, false, 0, false),
        ]);
        assert_eq!(
            state.next_history(now + HISTORY_GAP, false).as_deref(),
            Some("b"),
            "the chat that already had its page waits"
        );
        state.set_chats(&[
            chat("b", 20, false, 0, false),
            chat("new", 40, false, 0, false),
        ]);
        assert!(
            !state.queue.contains(&"a".to_owned()),
            "a chat that is gone is not asked again"
        );
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
        // Both chats are targets: a failed one has somewhere to go back to.
        state.configure(HistoryPrefetch::RecentAndPinned, None);
        state.set_chats(&[chat("a", 2, false, 0, false), chat("b", 1, false, 0, false)]);
        let now = Instant::now();
        assert!(state.next_history(now, true).is_none());
        assert_eq!(state.next_history(now, false).as_deref(), Some("a"));
        state.start_history("a".into());
        assert!(state.next_history(now, false).is_none());
        assert!(state.fail_history("a", now));
        assert!(state.next_history(now, false).is_none());
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(30), false)
                .as_deref(),
            Some("b"),
            "the failed chat went to the back"
        );
        state.start_history("b".into());
        assert!(state.finish_history("b", false, now + Duration::from_secs(30)));
        assert!(
            state
                .next_history(now + Duration::from_secs(31), false)
                .is_none(),
            "the gap after a page still holds"
        );
        assert_eq!(
            state
                .next_history(now + Duration::from_secs(51), false)
                .as_deref(),
            Some("a"),
            "and comes back after its backoff"
        );
    }

    #[test]
    fn a_foreign_history_ack_does_not_advance_the_queue() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        state.set_chats(&[chat("a", 1, false, 0, false)]);
        state.start_history("a".into());
        assert!(!state.finish_history("other", true, Instant::now()));
        assert!(
            state.next_history(Instant::now(), false).is_none(),
            "the request in flight still holds the turn"
        );
    }

    /// The chat that was open can be gone: a deleted chat must not keep the
    /// pump asking the phone for history nobody can read.
    #[test]
    fn a_focused_chat_that_is_gone_is_not_asked_for() {
        let chats = vec![chat("a", 1, false, 0, false)];
        assert_eq!(
            targets(&chats, HistoryPrefetch::Focused, Some("gone")),
            Vec::<ChatId>::new()
        );
        assert_eq!(
            targets(&chats, HistoryPrefetch::Focused, Some("a")),
            vec!["a".to_owned()]
        );
    }

    /// Turning the mode off stops the pump and empties the queue.
    #[test]
    fn turning_the_mode_off_stops_the_queue() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::RecentAndPinned, None);
        state.set_chats(&[chat("a", 1, false, 0, false)]);
        let now = Instant::now();
        assert_eq!(state.next_history(now, false).as_deref(), Some("a"));
        state.configure(HistoryPrefetch::Off, None);
        assert!(state.next_history(now, false).is_none());
    }

    /// A relink is a new session: the pages a chat was asked for are not
    /// carried over, so the configured mode starts from the top again.
    #[test]
    fn a_relink_forgets_the_pages_of_the_session() {
        let mut state = State::default();
        state.configure(HistoryPrefetch::Focused, Some("a".into()));
        state.set_chats(&[chat("a", 1, false, 0, false)]);
        let now = Instant::now();
        for _ in 0..HISTORY_MAX_PAGES {
            state.start_history("a".into());
            assert!(state.finish_history("a", true, now));
        }
        assert!(state.next_history(now + HISTORY_GAP, false).is_none());
        state.reset_session();
        assert_eq!(state.mode, HistoryPrefetch::Focused);
        assert_eq!(state.focused.as_deref(), Some("a"));
        assert!(state.needs_chats(), "the queue is built again");
        state.set_chats(&[chat("a", 1, false, 0, false)]);
        assert_eq!(
            state.next_history(now + HISTORY_GAP, false).as_deref(),
            Some("a"),
            "the new session may ask again"
        );
    }
}
