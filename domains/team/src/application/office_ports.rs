//! What the engine needs from the store for the office (ADR 0121): the room's items and the
//! members' games of chess.

use super::ports::Stored;
use crate::domain::{MemberId, Millis, chess, office};

/// The office's part of HQ's store.
pub trait OfficeStore {
    // ---- the office (ADR 0119) ----------------------------------------------------------

    /// Every item in the office, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn office_items(&self) -> Stored<Vec<office::Item>>;
    /// Puts a new item in the office; returns its number.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_office_item(
        &self,
        kind: office::Kind,
        x: i64,
        y: i64,
        by: &str,
        at: Millis,
    ) -> Stored<i64>;
    /// Moves an item.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn move_office_item(&self, id: i64, x: i64, y: i64) -> Stored<()>;
    /// Takes an item away.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn remove_office_item(&self, id: i64) -> Stored<()>;
    /// Makes the office exactly these items, numbers kept: for an undo and a reset.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn replace_office_items(&self, items: &[office::Item]) -> Stored<()>;
    // ---- games in the office (ADR 0121) ---------------------------------------------------

    /// Starts a game of chess, white to move; returns its number.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_chess_game(&self, white: MemberId, black: MemberId, at: Millis) -> Stored<i64>;
    /// Every game of chess, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn chess_games(&self) -> Stored<Vec<chess::Game>>;
    /// Keeps a game of chess as it now stands: its moves, status and last move.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_chess_game(&self, game: &chess::Game) -> Stored<()>;
}
