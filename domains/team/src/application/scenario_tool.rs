//! The `scenario` team tool (ADR 0135): look at the chess match a turn's task is, move in it,
//! or resign it. The rules are in [`crate::domain::chess_match`] and the engine's side is in
//! `engine/scenario.rs`; this is only the tool's schema and how its actions are read.

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};

/// The tool, offered only in a turn whose task is in a match.
pub(super) fn tool() -> Value {
    json!({
        "name": "scenario",
        "description": "Play your chess match, a move a turn, by the standard rules. look: the board, the moves so far and whose move it is. move: play one, in standard notation (san). An illegal move is refused, and is kept. resign: give the match up, and the other side wins. After your move your turn ends; you are woken when it is your move again. A side that stops moving forfeits. The world ends the match by the rules, and closes it.",
        "inputSchema": { "type": "object", "properties": {
            "action": { "type": "string", "enum": ["look", "move", "resign"] },
            "san": { "type": "string", "description": "for move: the move in standard algebraic notation" },
        }, "required": ["action"] },
    })
}

/// Runs one action of the tool for a turn.
pub(super) fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    let action = args["action"]
        .as_str()
        .ok_or("`action` is look, move or resign")?;
    engine
        .scenario_in(run.id, action, args["san"].as_str())
        .map_err(|e| e.to_string())
}
