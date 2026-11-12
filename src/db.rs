//! SQLite storage. Every query takes `guild_id` and filters by it.

use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sell,
    Haul,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Sell => "sell",
            Kind::Haul => "haul",
        }
    }

    fn parse(s: &str) -> Kind {
        if s == "sell" { Kind::Sell } else { Kind::Haul }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Sell => "Sell",
            Kind::Haul => "Haul",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: i64,
    pub guild_id: u64,
    pub kind: Kind,
    pub item: String,
    pub qty: i64,
    /// Total ISK. Always `Some` for Sell, always `None` for Haul.
    pub price: Option<i64>,
    pub note: Option<String>,
    pub poster_id: u64,
}

pub struct NewOrder<'a> {
    pub guild_id: u64,
    pub kind: Kind,
    pub item: &'a str,
    pub qty: i64,
    pub price: Option<i64>,
    pub note: Option<&'a str>,
    pub poster_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Board {
    pub channel_id: u64,
    pub message_id: Option<u64>,
}

#[derive(Debug)]
pub enum AcceptOutcome {
    Accepted(Order),
    /// Order does not exist in this guild (already accepted, deleted, or wrong id).
    Gone,
    OwnOrder,
}

pub struct Db {
    conn: Mutex<Connection>,
}

// Discord ids are u64; SQLite integers are i64. Store the bit pattern.
fn to_sql(id: u64) -> i64 {
    id as i64
}
fn from_sql(id: i64) -> u64 {
    id as u64
}

const ORDER_COLS: &str = "id, guild_id, kind, item, qty, price, note, poster_id";

fn order_from_row(row: &Row) -> rusqlite::Result<Order> {
    Ok(Order {
        id: row.get(0)?,
        guild_id: from_sql(row.get(1)?),
        kind: Kind::parse(&row.get::<_, String>(2)?),
        item: row.get(3)?,
        qty: row.get(4)?,
        price: row.get(5)?,
        note: row.get(6)?,
        poster_id: from_sql(row.get(7)?),
    })
}

impl Db {
    pub fn open(path: &str) -> Result<Db> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS orders (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                guild_id   INTEGER NOT NULL,
                kind       TEXT    NOT NULL CHECK (kind IN ('sell', 'haul')),
                item       TEXT    NOT NULL,
                qty        INTEGER NOT NULL DEFAULT 1 CHECK (qty >= 1),
                price      INTEGER,
                note       TEXT,
                poster_id  INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS orders_guild ON orders (guild_id, kind, id);
            CREATE TABLE IF NOT EXISTS boards (
                guild_id   INTEGER PRIMARY KEY,
                channel_id INTEGER NOT NULL,
                message_id INTEGER
            );",
        )?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert_order(&self, o: &NewOrder) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO orders (guild_id, kind, item, qty, price, note, poster_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                to_sql(o.guild_id),
                o.kind.as_str(),
                o.item,
                o.qty,
                o.price,
                o.note,
                to_sql(o.poster_id)
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// All open orders in a guild, oldest first.
    pub fn list_orders(&self, guild_id: u64) -> Result<Vec<Order>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ORDER_COLS} FROM orders WHERE guild_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([to_sql(guild_id)], order_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_order(&self, guild_id: u64, id: i64) -> Result<Option<Order>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                &format!("SELECT {ORDER_COLS} FROM orders WHERE guild_id = ?1 AND id = ?2"),
                params![to_sql(guild_id), id],
                order_from_row,
            )
            .optional()?)
    }

    /// Removes the order if it still exists and was not posted by `acceptor_id`.
    /// A single DELETE ... RETURNING, so of two concurrent accepts only one gets the row.
    pub fn accept_order(&self, guild_id: u64, id: i64, acceptor_id: u64) -> Result<AcceptOutcome> {
        let conn = self.conn();
        let taken = conn
            .query_row(
                &format!(
                    "DELETE FROM orders WHERE guild_id = ?1 AND id = ?2 AND poster_id != ?3
                     RETURNING {ORDER_COLS}"
                ),
                params![to_sql(guild_id), id, to_sql(acceptor_id)],
                order_from_row,
            )
            .optional()?;
        if let Some(order) = taken {
            return Ok(AcceptOutcome::Accepted(order));
        }
        let own: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM orders WHERE guild_id = ?1 AND id = ?2 AND poster_id = ?3)",
            params![to_sql(guild_id), id, to_sql(acceptor_id)],
            |r| r.get(0),
        )?;
        Ok(if own { AcceptOutcome::OwnOrder } else { AcceptOutcome::Gone })
    }

    /// Deletes the order only if `poster_id` posted it. Returns the deleted order.
    pub fn delete_own_order(&self, guild_id: u64, id: i64, poster_id: u64) -> Result<Option<Order>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                &format!(
                    "DELETE FROM orders WHERE guild_id = ?1 AND id = ?2 AND poster_id = ?3
                     RETURNING {ORDER_COLS}"
                ),
                params![to_sql(guild_id), id, to_sql(poster_id)],
                order_from_row,
            )
            .optional()?)
    }

    pub fn get_board(&self, guild_id: u64) -> Result<Option<Board>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT channel_id, message_id FROM boards WHERE guild_id = ?1",
                [to_sql(guild_id)],
                |r| {
                    Ok(Board {
                        channel_id: from_sql(r.get(0)?),
                        message_id: r.get::<_, Option<i64>>(1)?.map(from_sql),
                    })
                },
            )
            .optional()?)
    }

    pub fn set_board(&self, guild_id: u64, board: Board) -> Result<()> {
        self.conn().execute(
            "INSERT INTO boards (guild_id, channel_id, message_id) VALUES (?1, ?2, ?3)
             ON CONFLICT (guild_id) DO UPDATE SET channel_id = ?2, message_id = ?3",
            params![
                to_sql(guild_id),
                to_sql(board.channel_id),
                board.message_id.map(to_sql)
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sell(db: &Db, guild: u64, poster: u64, item: &str) -> i64 {
        db.insert_order(&NewOrder {
            guild_id: guild,
            kind: Kind::Sell,
            item,
            qty: 1,
            price: Some(1_200_000_000),
            note: None,
            poster_id: poster,
        })
        .unwrap()
    }

    #[test]
    fn orders_are_scoped_by_guild() {
        let db = Db::open_in_memory().unwrap();
        let a = sell(&db, 1, 10, "Paladin");
        sell(&db, 2, 10, "Vexor");
        let list = db.list_orders(1).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].item, "Paladin");
        assert!(db.get_order(2, a).unwrap().is_none());
        assert!(matches!(db.accept_order(2, a, 20).unwrap(), AcceptOutcome::Gone));
        assert!(db.delete_own_order(2, a, 10).unwrap().is_none());
        assert!(db.get_order(1, a).unwrap().is_some());
    }

    #[test]
    fn accept_only_once_and_not_own() {
        let db = Db::open_in_memory().unwrap();
        let id = sell(&db, 1, 10, "Paladin");
        assert!(matches!(db.accept_order(1, id, 10).unwrap(), AcceptOutcome::OwnOrder));
        assert!(matches!(db.accept_order(1, id, 20).unwrap(), AcceptOutcome::Accepted(_)));
        assert!(matches!(db.accept_order(1, id, 30).unwrap(), AcceptOutcome::Gone));
        assert!(db.list_orders(1).unwrap().is_empty());
    }

    #[test]
    fn concurrent_accepts_have_one_winner() {
        let db = std::sync::Arc::new(Db::open_in_memory().unwrap());
        let id = sell(&db, 1, 10, "Paladin");
        let handles: Vec<_> = (0..16)
            .map(|i| {
                let db = db.clone();
                std::thread::spawn(move || db.accept_order(1, id, 100 + i).unwrap())
            })
            .collect();
        let wins = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|o| matches!(o, AcceptOutcome::Accepted(_)))
            .count();
        assert_eq!(wins, 1);
    }

    #[test]
    fn only_poster_can_delete() {
        let db = Db::open_in_memory().unwrap();
        let id = sell(&db, 1, 10, "Paladin");
        assert!(db.delete_own_order(1, id, 20).unwrap().is_none());
        assert!(db.delete_own_order(1, id, 10).unwrap().is_some());
        assert!(db.list_orders(1).unwrap().is_empty());
    }

    #[test]
    fn large_discord_ids_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let guild = u64::MAX - 5;
        let id = sell(&db, guild, u64::MAX - 1, "Paladin");
        let o = db.get_order(guild, id).unwrap().unwrap();
        assert_eq!(o.guild_id, guild);
        assert_eq!(o.poster_id, u64::MAX - 1);
        db.set_board(guild, Board { channel_id: u64::MAX, message_id: Some(7) }).unwrap();
        let b = db.get_board(guild).unwrap().unwrap();
        assert_eq!(b.channel_id, u64::MAX);
        assert_eq!(b.message_id, Some(7));
    }
}
