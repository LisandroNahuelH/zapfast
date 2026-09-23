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
    /// Every pin of every chip that keeps its own order.
    pub fn chip_pins(&self) -> Result<Vec<(String, String, i64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT chip, chat, pinned_at FROM chip_pins")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    }

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

    /// Writes the order the user dragged a chip's pins into. Only the local
    /// order changes: nothing goes to WhatsApp.
    pub fn reorder_chip_pins(&self, chip: &str, order: &[String]) -> Result<()> {
        let mut current = Vec::new();
        {
            let mut statement = self.connection.prepare(
                "SELECT chat FROM chip_pins WHERE chip = ?1 ORDER BY pinned_at DESC, chat",
            )?;
            let rows = statement.query_map(params![chip], |row| row.get::<_, String>(0))?;
            for row in rows {
                current.push(row?);
            }
        }
        // Only the chats still pinned, in the order asked for; anything the
        // list did not name keeps its place at the end.
        let mut wanted: Vec<String> = order
            .iter()
            .filter(|id| current.iter().any(|known| known == *id))
            .cloned()
            .collect();
        for known in current {
            if !wanted.contains(&known) {
                wanted.push(known);
            }
        }
        let base = jiff::Timestamp::now().as_millisecond();
        let count = wanted.len() as i64;
        for (index, chat) in wanted.iter().enumerate() {
            let stamp = base + count - index as i64;
            self.connection.execute(
                "UPDATE chip_pins SET pinned_at = ?3 WHERE chip = ?1 AND chat = ?2",
                params![chip, chat, stamp],
            )?;
        }
        Ok(())
    }

    /// Forgets every pin of a chip. Used when a chat leaves the archive.
    pub fn clear_chip_pins(&self, chat: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM chip_pins WHERE chat = ?1", params![chat])?;
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
            .reorder_chip_pins("favorites", &["a@s.whatsapp.net".into()])
            .expect("reorder");
        archive
            .set_chip_pinned("favorites", "a@s.whatsapp.net", false)
            .expect("unpin");
        let pins = archive.chip_pins().expect("pins");
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "groups");
        archive.clear_chip_pins("b@g.us").expect("clear");
        assert!(archive.chip_pins().expect("pins").is_empty());
    }
}
