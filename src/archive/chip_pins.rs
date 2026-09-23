//! Pins that belong to one chat-list chip.
//!
//! `All` keeps the WhatsApp pin in `chats.pinned`, so the phone and the other
//! linked devices stay in step. Every other chip keeps its own order here, so
//! pinning a chat in Favorites leaves `All` alone. Nothing in this table
//! reaches WhatsApp.

use rusqlite::params;

use super::{Archive, Result};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS chip_pins (
    chip TEXT NOT NULL,
    chat TEXT NOT NULL,
    pinned_at INTEGER NOT NULL,
    PRIMARY KEY (chip, chat)
);
";

impl Archive {
    /// Every pin of every chip that keeps its own order, newest first.
    pub fn chip_pins(&self) -> Result<Vec<(String, String, i64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT chip, chat, pinned_at FROM chip_pins")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    }

    /// Pins a chat inside one chip, or takes the pin off. The stamp is what
    /// orders the chip, so pinning the same chat again moves it to the top.
    pub fn set_chip_pinned(&self, chip: &str, chat: &str, pinned: bool) -> Result<()> {
        if pinned {
            let at = jiff::Timestamp::now().as_millisecond();
            self.connection.execute(
                "INSERT INTO chip_pins (chip, chat, pinned_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(chip, chat) DO UPDATE SET pinned_at = excluded.pinned_at",
                params![chip, chat, at],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM chip_pins WHERE chip = ?1 AND chat = ?2",
                params![chip, chat],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::archive::Archive;

    #[test]
    fn a_chip_keeps_its_own_pin_order() {
        let archive = Archive::in_memory().expect("opens");
        archive
            .ensure_chat("a@s.whatsapp.net", "Ada")
            .expect("chat");
        archive.ensure_chat("b@g.us", "Group").expect("chat");
        archive
            .set_chip_pinned("favorites", "a@s.whatsapp.net", true)
            .expect("pin");
        archive
            .set_chip_pinned("groups", "b@g.us", true)
            .expect("pin");
        // Pinning inside a chip never touches the WhatsApp pin.
        let ada = archive.chat("a@s.whatsapp.net").expect("row").expect("ada");
        assert!(!ada.pinned);
        assert_eq!(archive.chip_pins().expect("pins").len(), 2);
        archive
            .set_chip_pinned("favorites", "a@s.whatsapp.net", false)
            .expect("unpin");
        let pins = archive.chip_pins().expect("pins");
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "groups");
        // A chat that leaves the archive takes its pins with it.
        archive.delete_chat("b@g.us").expect("delete");
        assert!(archive.chip_pins().expect("pins").is_empty());
        // Unlinking drops them too, so a later link starts clean.
        archive
            .set_chip_pinned("favorites", "a@s.whatsapp.net", true)
            .expect("pin");
        archive.clear().expect("clear");
        assert!(archive.chip_pins().expect("pins").is_empty());
    }

    #[test]
    fn a_deleted_label_takes_its_chip_pins_with_it() {
        let archive = Archive::in_memory().expect("opens");
        archive
            .ensure_chat("a@s.whatsapp.net", "Ada")
            .expect("chat");
        let label = archive
            .create_label("Work", "#3b82f6", 1)
            .expect("create")
            .expect("created");
        let chip = crate::model::ChatFilter::label_key(&label.id);
        archive
            .set_chip_pinned(&chip, "a@s.whatsapp.net", true)
            .expect("pin");
        assert_eq!(archive.chip_pins().expect("pins").len(), 1);
        // Label ids come from the clock, so a label recreated in the same
        // second would inherit whatever this one leaves behind.
        assert!(archive.delete_label(&label.id).expect("delete"));
        assert!(
            archive.chip_pins().expect("pins").is_empty(),
            "the label's own pins go with the label"
        );
    }
}
