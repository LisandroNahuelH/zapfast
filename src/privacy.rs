//! WhatsApp account privacy: last seen, online, photo, about, groups,
//! receipts, calls, and who can message. Values live on the phone, not in
//! `settings.json`.

use std::collections::HashMap;

use whatsapp_rust::wacore::iq::privacy::{PrivacyCategory, PrivacySettingsResponse, PrivacyValue};

use crate::model::ChatId;

/// Account privacy category shown in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrivacyKind {
    LastSeen,
    Online,
    Profile,
    About,
    GroupAdd,
    ReadReceipts,
    CallAdd,
    Messages,
}

impl PrivacyKind {
    pub const ALL: [Self; 8] = [
        Self::LastSeen,
        Self::Online,
        Self::Profile,
        Self::About,
        Self::GroupAdd,
        Self::ReadReceipts,
        Self::CallAdd,
        Self::Messages,
    ];

    /// Categories that accept an Except list.
    pub const EXCEPT: [Self; 4] = [Self::LastSeen, Self::Profile, Self::About, Self::GroupAdd];

    pub fn label(self) -> &'static str {
        match self {
            Self::LastSeen => "Last seen",
            Self::Online => "Online",
            Self::Profile => "Profile photo",
            Self::About => "About",
            Self::GroupAdd => "Who can add me to groups",
            Self::ReadReceipts => "Read receipts",
            Self::CallAdd => "Who can call me",
            Self::Messages => "Who can message me",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::LastSeen => "When people can see you were last using WhatsApp.",
            Self::Online => "When people can see you are online now.",
            Self::Profile => "Who can see your profile photo.",
            Self::About => "Who can see your About text. This is not the Status tab.",
            Self::GroupAdd => "Who can add you to a group.",
            Self::ReadReceipts => {
                "Everyone or nobody on this WhatsApp account. The Chats switch still applies to this copy."
            }
            Self::CallAdd => "Who can call you on WhatsApp.",
            Self::Messages => "Who can start a chat with you.",
        }
    }

    pub fn except_title(self) -> &'static str {
        match self {
            Self::LastSeen => "Hide last seen from",
            Self::Profile => "Hide profile photo from",
            Self::About => "Hide About from",
            Self::GroupAdd => "Who cannot add you to groups",
            _ => "Except",
        }
    }

    pub fn choices(self) -> &'static [PrivacyChoice] {
        match self {
            Self::LastSeen | Self::Profile | Self::About | Self::GroupAdd => &[
                PrivacyChoice::Everyone,
                PrivacyChoice::Contacts,
                PrivacyChoice::Except,
                PrivacyChoice::Nobody,
            ],
            Self::Online => &[PrivacyChoice::Everyone, PrivacyChoice::SameAsLastSeen],
            Self::ReadReceipts => &[PrivacyChoice::Everyone, PrivacyChoice::Nobody],
            Self::CallAdd => &[
                PrivacyChoice::Everyone,
                PrivacyChoice::Contacts,
                PrivacyChoice::ContactsAndKnown,
            ],
            Self::Messages => &[PrivacyChoice::Everyone, PrivacyChoice::Contacts],
        }
    }

    pub fn allows_except(self) -> bool {
        Self::EXCEPT.contains(&self)
    }

    pub fn wire_name(self) -> &'static str {
        match self {
            Self::LastSeen => "last",
            Self::Online => "online",
            Self::Profile => "profile",
            Self::About => "status",
            Self::GroupAdd => "groupadd",
            Self::ReadReceipts => "readreceipts",
            Self::CallAdd => "calladd",
            Self::Messages => "messages",
        }
    }

    pub fn from_wire(category: &PrivacyCategory) -> Option<Self> {
        match category {
            PrivacyCategory::Last => Some(Self::LastSeen),
            PrivacyCategory::Online => Some(Self::Online),
            PrivacyCategory::Profile => Some(Self::Profile),
            PrivacyCategory::Status => Some(Self::About),
            PrivacyCategory::GroupAdd => Some(Self::GroupAdd),
            PrivacyCategory::ReadReceipts => Some(Self::ReadReceipts),
            PrivacyCategory::CallAdd => Some(Self::CallAdd),
            PrivacyCategory::Messages => Some(Self::Messages),
            PrivacyCategory::DefenseMode | PrivacyCategory::Other(_) => None,
        }
    }

    pub fn to_wire(self) -> PrivacyCategory {
        match self {
            Self::LastSeen => PrivacyCategory::Last,
            Self::Online => PrivacyCategory::Online,
            Self::Profile => PrivacyCategory::Profile,
            Self::About => PrivacyCategory::Status,
            Self::GroupAdd => PrivacyCategory::GroupAdd,
            Self::ReadReceipts => PrivacyCategory::ReadReceipts,
            Self::CallAdd => PrivacyCategory::CallAdd,
            Self::Messages => PrivacyCategory::Messages,
        }
    }
}

/// One picker value. Not every value is valid for every category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrivacyChoice {
    Everyone,
    Contacts,
    Except,
    Nobody,
    SameAsLastSeen,
    ContactsAndKnown,
}

impl PrivacyChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Everyone => "Everyone",
            Self::Contacts => "My contacts",
            Self::Except => "Except…",
            Self::Nobody => "Nobody",
            Self::SameAsLastSeen => "Same as last seen",
            Self::ContactsAndKnown => "My contacts and other people with my number",
        }
    }

    pub fn from_wire(value: &PrivacyValue) -> Option<Self> {
        match value {
            PrivacyValue::All => Some(Self::Everyone),
            PrivacyValue::Contacts => Some(Self::Contacts),
            PrivacyValue::ContactBlacklist => Some(Self::Except),
            PrivacyValue::None => Some(Self::Nobody),
            PrivacyValue::MatchLastSeen => Some(Self::SameAsLastSeen),
            PrivacyValue::Known => Some(Self::ContactsAndKnown),
            PrivacyValue::Off | PrivacyValue::OnStandard | PrivacyValue::Other(_) => None,
        }
    }

    pub fn to_wire(self) -> Option<PrivacyValue> {
        match self {
            Self::Everyone => Some(PrivacyValue::All),
            Self::Contacts => Some(PrivacyValue::Contacts),
            Self::Except => Some(PrivacyValue::ContactBlacklist),
            Self::Nobody => Some(PrivacyValue::None),
            Self::SameAsLastSeen => Some(PrivacyValue::MatchLastSeen),
            Self::ContactsAndKnown => Some(PrivacyValue::Known),
        }
    }
}

/// Exclusion list for one Except category.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivacyList {
    pub dhash: String,
    pub ids: Vec<ChatId>,
}

/// Last confirmed account snapshot, plus in-flight revert.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub values: HashMap<PrivacyKind, PrivacyChoice>,
    pub lists: HashMap<PrivacyKind, PrivacyList>,
    pub revert: HashMap<PrivacyKind, (PrivacyChoice, PrivacyList)>,
    pub fetch_failed: bool,
    pub loaded: bool,
}

impl Snapshot {
    pub fn get(&self, kind: PrivacyKind) -> Option<PrivacyChoice> {
        self.values.get(&kind).copied()
    }

    pub fn list(&self, kind: PrivacyKind) -> PrivacyList {
        self.lists.get(&kind).cloned().unwrap_or_default()
    }

    pub fn pending(&self, kind: PrivacyKind) -> bool {
        self.revert.contains_key(&kind)
    }

    pub fn apply_fetch(
        &mut self,
        values: Vec<(PrivacyKind, PrivacyChoice)>,
        lists: Vec<(PrivacyKind, PrivacyList)>,
        failed: bool,
    ) {
        self.fetch_failed = failed;
        if failed && self.loaded {
            return;
        }
        if !failed {
            self.values = values.into_iter().collect();
            self.lists = lists.into_iter().collect();
            self.loaded = true;
            self.revert.clear();
        }
    }

    pub fn begin_set(&mut self, kind: PrivacyKind, choice: PrivacyChoice) {
        let previous = self.get(kind).unwrap_or(kind.choices()[0]);
        let list = self.list(kind);
        self.revert.entry(kind).or_insert((previous, list));
        self.values.insert(kind, choice);
    }

    pub fn finish_set(
        &mut self,
        kind: PrivacyKind,
        dhash: Option<String>,
        ids: Option<Vec<ChatId>>,
    ) {
        self.revert.remove(&kind);
        if let Some(ids) = ids {
            self.lists.entry(kind).or_default().ids = ids;
        }
        if let Some(dhash) = dhash {
            self.lists.entry(kind).or_default().dhash = dhash;
        }
    }

    pub fn fail_set(&mut self, kind: PrivacyKind) {
        if let Some((choice, list)) = self.revert.remove(&kind) {
            self.values.insert(kind, choice);
            self.lists.insert(kind, list);
        }
    }

    /// Sample values for the demo Settings page.
    pub fn demo(except: ChatId) -> Self {
        let mut snapshot = Self {
            loaded: true,
            ..Self::default()
        };
        snapshot
            .values
            .insert(PrivacyKind::LastSeen, PrivacyChoice::Except);
        snapshot
            .values
            .insert(PrivacyKind::Online, PrivacyChoice::SameAsLastSeen);
        snapshot
            .values
            .insert(PrivacyKind::Profile, PrivacyChoice::Contacts);
        snapshot
            .values
            .insert(PrivacyKind::About, PrivacyChoice::Everyone);
        snapshot
            .values
            .insert(PrivacyKind::GroupAdd, PrivacyChoice::Contacts);
        snapshot
            .values
            .insert(PrivacyKind::ReadReceipts, PrivacyChoice::Everyone);
        snapshot
            .values
            .insert(PrivacyKind::CallAdd, PrivacyChoice::Contacts);
        snapshot
            .values
            .insert(PrivacyKind::Messages, PrivacyChoice::Everyone);
        snapshot.lists.insert(
            PrivacyKind::LastSeen,
            PrivacyList {
                dhash: "demo".into(),
                ids: vec![except],
            },
        );
        snapshot
    }
}

pub fn values_from_response(
    settings: &PrivacySettingsResponse,
) -> Vec<(PrivacyKind, PrivacyChoice)> {
    let mut out = Vec::new();
    for setting in &settings.settings {
        let Some(kind) = PrivacyKind::from_wire(&setting.category) else {
            continue;
        };
        let Some(choice) = PrivacyChoice::from_wire(&setting.value) else {
            continue;
        };
        if !kind.choices().contains(&choice) {
            continue;
        }
        out.push((kind, choice));
    }
    out
}

pub fn wire_set(
    kind: PrivacyKind,
    choice: PrivacyChoice,
) -> Option<(PrivacyCategory, PrivacyValue)> {
    if !kind.choices().contains(&choice) {
        return None;
    }
    let value = choice.to_wire()?;
    let category = kind.to_wire();
    category.is_valid_value(&value).then_some((category, value))
}

/// Parse MEX `get_privacy_lists` data. Category order matches [`PrivacyKind::EXCEPT`].
pub fn lists_from_mex(data: &serde_json::Value) -> Vec<(PrivacyKind, PrivacyList)> {
    let parsed: whatsapp_rust::wacore::iq::mex_operations::get_privacy_lists::Response =
        match serde_json::from_value(data.clone()) {
            Ok(parsed) => parsed,
            Err(_) => return Vec::new(),
        };
    let users = parsed.xwa2_fetch_wa_users.unwrap_or_default();
    let mut out = Vec::new();
    for (index, user) in users.into_iter().enumerate() {
        let Some(list) = user.privacy_contact_list else {
            continue;
        };
        let kind = user
            .id
            .as_deref()
            .and_then(kind_from_wire_name)
            .or_else(|| PrivacyKind::EXCEPT.get(index).copied());
        let Some(kind) = kind else {
            continue;
        };
        let ids = list
            .contacts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|contact| contact.pn_jid.or(contact.jid))
            .filter(|id| !id.is_empty())
            .collect();
        out.push((
            kind,
            PrivacyList {
                dhash: list.dhash.unwrap_or_default(),
                ids,
            },
        ));
    }
    out
}

fn kind_from_wire_name(name: &str) -> Option<PrivacyKind> {
    PrivacyKind::ALL
        .into_iter()
        .find(|kind| kind.wire_name() == name)
}

/// Whether a failed list SET should refetch MEX and retry once.
pub fn retry_list_after_conflict(code: u16, retried: bool) -> bool {
    code == 409 && !retried
}

pub fn except_diff(current: &[ChatId], picked: &[ChatId]) -> (Vec<ChatId>, Vec<ChatId>) {
    let add = picked
        .iter()
        .filter(|id| !current.iter().any(|known| known == *id))
        .cloned()
        .collect();
    let remove = current
        .iter()
        .filter(|id| !picked.iter().any(|known| known == *id))
        .cloned()
        .collect();
    (add, remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::wacore::iq::privacy::PrivacySetting;

    const ALL_CHOICES: [PrivacyChoice; 6] = [
        PrivacyChoice::Everyone,
        PrivacyChoice::Contacts,
        PrivacyChoice::Except,
        PrivacyChoice::Nobody,
        PrivacyChoice::SameAsLastSeen,
        PrivacyChoice::ContactsAndKnown,
    ];

    #[test]
    fn each_category_accepts_only_its_values() {
        for kind in PrivacyKind::ALL {
            for choice in ALL_CHOICES {
                let wire = wire_set(kind, choice);
                if kind.choices().contains(&choice) {
                    assert!(wire.is_some(), "{kind:?} {choice:?}");
                } else {
                    assert!(wire.is_none(), "{kind:?} {choice:?}");
                }
            }
        }
        assert!(wire_set(PrivacyKind::Online, PrivacyChoice::Nobody).is_none());
        assert!(wire_set(PrivacyKind::ReadReceipts, PrivacyChoice::Contacts).is_none());
        assert!(wire_set(PrivacyKind::CallAdd, PrivacyChoice::Except).is_none());
        assert!(wire_set(PrivacyKind::Messages, PrivacyChoice::Nobody).is_none());
    }

    #[test]
    fn fetch_maps_phone_categories_and_skips_defense() {
        let settings = PrivacySettingsResponse {
            settings: vec![
                PrivacySetting {
                    category: PrivacyCategory::Last,
                    value: PrivacyValue::ContactBlacklist,
                },
                PrivacySetting {
                    category: PrivacyCategory::Online,
                    value: PrivacyValue::MatchLastSeen,
                },
                PrivacySetting {
                    category: PrivacyCategory::ReadReceipts,
                    value: PrivacyValue::None,
                },
                PrivacySetting {
                    category: PrivacyCategory::DefenseMode,
                    value: PrivacyValue::Off,
                },
            ],
        };
        let values = values_from_response(&settings);
        assert_eq!(
            values,
            vec![
                (PrivacyKind::LastSeen, PrivacyChoice::Except),
                (PrivacyKind::Online, PrivacyChoice::SameAsLastSeen),
                (PrivacyKind::ReadReceipts, PrivacyChoice::Nobody),
            ]
        );
    }

    #[test]
    fn mex_lists_keep_dhash_and_prefer_phone_jid() {
        let data = serde_json::json!({
            "xwa2_fetch_wa_users": [
                {
                    "id": "last",
                    "privacy_contact_list": {
                        "dhash": "abc",
                        "contacts": [
                            {"jid": "123@lid", "pn_jid": "393331234567@s.whatsapp.net"}
                        ]
                    }
                }
            ]
        });
        let lists = lists_from_mex(&data);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].0, PrivacyKind::LastSeen);
        assert_eq!(lists[0].1.dhash, "abc");
        assert_eq!(lists[0].1.ids, vec!["393331234567@s.whatsapp.net"]);
    }

    #[test]
    fn except_diff_splits_add_and_remove() {
        let current = vec!["a".into(), "b".into()];
        let picked = vec!["b".into(), "c".into()];
        let (add, remove) = except_diff(&current, &picked);
        assert_eq!(add, vec!["c"]);
        assert_eq!(remove, vec!["a"]);
    }

    #[test]
    fn conflict_retries_once() {
        assert!(retry_list_after_conflict(409, false));
        assert!(!retry_list_after_conflict(409, true));
        assert!(!retry_list_after_conflict(500, false));
    }

    #[test]
    fn failed_set_restores_the_snapshot() {
        let mut snapshot = Snapshot::demo("ada".into());
        snapshot.begin_set(PrivacyKind::Profile, PrivacyChoice::Nobody);
        assert_eq!(
            snapshot.get(PrivacyKind::Profile),
            Some(PrivacyChoice::Nobody)
        );
        snapshot.fail_set(PrivacyKind::Profile);
        assert_eq!(
            snapshot.get(PrivacyKind::Profile),
            Some(PrivacyChoice::Contacts)
        );
    }
}
