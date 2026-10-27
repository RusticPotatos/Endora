//! The `chess` team tool (ADR 0121): look at the member's game, play a move in it, or
//! challenge another member to one. The rules are in [`crate::domain::chess`] and the engine's
//! side is in `engine/games.rs`; this is only the tool's schema and how its actions are read.

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};

/// The tool, as every role is offered it.
pub(super) fn tool() -> Value {
    json!({
        "name": "chess",
        "description": "Chess with a teammate, by the rules, played a move at a time in your downtime turns. board: your game's position (FEN and drawn), whose move it is and the last moves; you may look any time. move: play one move in standard notation (san) when it is your move; refused if it is not legal. challenge: start a game with a member who is at hand (member); you have white. One game each. Moves and challenges are made in turns on the house's own brains, never on Claude; a game goes on over days, and when it is your move you are given a short turn for it.",
        "inputSchema": { "type": "object", "properties": {
            "action": { "type": "string", "enum": ["board", "move", "challenge"] },
            "san": { "type": "string", "description": "for move: the move in standard algebraic notation" },
            "member": { "type": "string", "description": "for challenge: the teammate's name" },
        }, "required": ["action"] },
    })
}

fn text<'a>(args: &'a Value, key: &str, what: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| format!("`{key}` is needed: {what}"))
}

/// Runs one action of the tool for a turn.
pub(super) fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    match args["action"].as_str() {
        Some("board") => engine.chess_board_in(run.id).map_err(|e| e.to_string()),
        Some("move") => {
            let san = text(
                args,
                "san",
                "the move in standard notation, such as e4 or Nf3",
            )?;
            engine.chess_move_in(run.id, san).map_err(|e| e.to_string())
        }
        Some("challenge") => {
            let member = text(args, "member", "the name of the teammate to challenge")?;
            engine
                .chess_challenge_in(run.id, member)
                .map_err(|e| e.to_string())
        }
        _ => Err("`action` is board, move or challenge".to_owned()),
    }
}
