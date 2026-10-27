//! The office in HQ's store: the room's items and the members' games of chess.

use rusqlite::params;

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::office_ports::OfficeStore;
use crate::application::ports::Stored;
use crate::domain::{MemberId, Millis, chess, office};

impl OfficeStore for SqliteTeamStore {
    fn office_items(&self) -> Stored<Vec<office::Item>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, kind, x, y, placed_by, placed_at FROM office_items ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(backend)?;
        let mut items = Vec::new();
        for row in rows {
            let (id, kind, x, y, placed_by, placed_at) = row.map_err(backend)?;
            let kind = office::Kind::from_name(&kind).ok_or_else(|| corrupt("an item's kind"))?;
            items.push(office::Item {
                id,
                kind,
                x,
                y,
                placed_by,
                placed_at,
            });
        }
        Ok(items)
    }

    fn add_office_item(
        &self,
        kind: office::Kind,
        x: i64,
        y: i64,
        by: &str,
        at: Millis,
    ) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO office_items (kind, x, y, placed_by, placed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind.name(), x, y, by, at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn move_office_item(&self, id: i64, x: i64, y: i64) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE office_items SET x = ?2, y = ?3 WHERE id = ?1",
                params![id, x, y],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn remove_office_item(&self, id: i64) -> Stored<()> {
        self.conn()?
            .execute("DELETE FROM office_items WHERE id = ?1", [id])
            .map_err(backend)?;
        Ok(())
    }

    fn replace_office_items(&self, items: &[office::Item]) -> Stored<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        tx.execute("DELETE FROM office_items", [])
            .map_err(backend)?;
        for item in items {
            tx.execute(
                "INSERT INTO office_items (id, kind, x, y, placed_by, placed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item.id,
                    item.kind.name(),
                    item.x,
                    item.y,
                    item.placed_by,
                    item.placed_at
                ],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)
    }

    fn add_chess_game(&self, white: MemberId, black: MemberId, at: Millis) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO chess_games (white, black, started_at, last_move_at) \
             VALUES (?1, ?2, ?3, ?3)",
            params![white.0, black.0, at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn chess_games(&self) -> Stored<Vec<chess::Game>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, white, black, moves, status, reason, started_at, last_move_at \
                 FROM chess_games ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            })
            .map_err(backend)?;
        let mut all = Vec::new();
        for row in rows {
            let (id, white, black, moves, status, reason, started_at, last_move_at) =
                row.map_err(backend)?;
            all.push(chess::Game {
                id,
                white: MemberId(white),
                black: MemberId(black),
                moves: moves.split_whitespace().map(str::to_owned).collect(),
                status: chess::Status::from_name(&status)
                    .ok_or_else(|| corrupt("a chess game's status"))?,
                reason,
                started_at,
                last_move_at,
            });
        }
        Ok(all)
    }

    fn save_chess_game(&self, game: &chess::Game) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE chess_games SET moves = ?2, status = ?3, reason = ?4, last_move_at = ?5 \
                 WHERE id = ?1",
                params![
                    game.id,
                    game.moves.join(" "),
                    game.status.name(),
                    game.reason,
                    game.last_move_at
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}
