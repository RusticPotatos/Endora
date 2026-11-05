//! A turn's prompt budget (ADR 0142, amended 2026-10-30): what a turn may read in all, in bytes,
//! before the worker ends it and HQ continues the item in a fresh turn. One pair of figures that
//! two sides must say alike: the team domain picks one by the member's brain and HQ judges the
//! turn by it, and the worker ends the turn at it.

/// What a house turn (the NAS's model, the Mac, DeepSeek) may read in all, in bytes: about
/// 60,000 tokens at four bytes a token. The Mac reads about 100 tokens a second, so that is
/// about ten minutes of reading in a turn; measured, a ten-call task turn read about 74,600
/// tokens with old results stubbed and work turns 110,000 to 200,000 (ADR 0142, amended
/// 2026-10-30).
pub const HOUSE_PROMPT_BUDGET: usize = 240_000;

/// What a Claude turn may read in all, in bytes: about 120,000 tokens, the measured ten-call
/// task turn with every result whole (about 119,500), so a turn of that length still ends on
/// its own and only the long ones (110,000 to 200,000 and more) go on fresh.
pub const CLAUDE_PROMPT_BUDGET: usize = 480_000;

/// About how many bytes of a prompt make a token.
pub const BYTES_A_TOKEN: usize = 4;
