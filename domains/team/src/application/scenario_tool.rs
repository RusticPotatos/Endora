//! The `scenario` team tool (the Holodeck plan's phase 5): look at the scenario a turn's task
//! is, or act in it. The world's rules are in [`crate::domain::scenario`] and the engine's side
//! is in `engine/scenario.rs`; this is only the tool's schema and how its actions are read.

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};

/// The tool, offered only in a turn whose task is in a scenario, for that world (`kind`).
pub(super) fn tool(kind: &str) -> Value {
    if kind == crate::domain::chess_match::CHESS {
        return json!({
            "name": "scenario",
            "description": "Play your chess match, a move a turn, by the standard rules. look: the board, the moves so far and whose move it is. move: play one, in standard notation (san). An illegal move is refused, and is kept. resign: give the match up, and the other side wins. After your move your turn ends; you are woken when it is your move again. A side that stops moving forfeits. The world ends the match by the rules, and closes it.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["look", "move", "resign"] },
                "san": { "type": "string", "description": "for move: the move in standard algebraic notation" },
            }, "required": ["action"] },
        });
    }
    if kind == crate::domain::station::SILENT_STATION {
        return json!({
            "name": "scenario",
            "description": "Act in Silent Station, as your role in the crew. look: what the crew has found and where things stand (free). scan_station (science): life signs, power and hazard; 1 tick, 10 energy. inspect_comms (engineer): open the comms junction; 2 ticks, 10 energy. analyze_anomaly (science): what the anomaly is; 2 ticks, 15 energy. route_power (engineer): bypass a dead relay; 2 ticks, 25 energy. send_probe (captain): a probe beside the array as a relay; 1 tick, 35 energy. attempt_contact (captain): call the station; 1 tick, 10 energy. plan (captain): post the crew's plan, who does what in which order (text); free. Only your role's actions are yours; a refused action costs nothing. The world says how the mission ends, and closes it itself.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["look", "plan", "scan_station", "inspect_comms", "analyze_anomaly", "route_power", "send_probe", "attempt_contact"] },
                "text": { "type": "string", "description": "for plan: the crew's plan, who does what, in which order" },
            }, "required": ["action"] },
        });
    }
    json!({
        "name": "scenario",
        "description": "Act in the scenario your task is. look: where things stand (free). inspect: open the lamp's housing and look at the bulb (5 minutes). test_circuit: whether current reaches the lamp (10 minutes, housing open). replace_part: fit a new part, bulb or fuse (15 minutes, housing open, each part once). A refused action costs nothing. The world says when it is solved or failed, and closes your task itself.",
        "inputSchema": { "type": "object", "properties": {
            "action": { "type": "string", "enum": ["look", "inspect", "test_circuit", "replace_part"] },
            "part": { "type": "string", "enum": ["bulb", "fuse"], "description": "for replace_part: the part to fit" },
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
        .ok_or("`action` is look, inspect, test_circuit or replace_part")?;
    // What the action is about: a lighthouse's part, or a match's move.
    let about = args["part"]
        .as_str()
        .or_else(|| args["san"].as_str())
        .or_else(|| args["text"].as_str());
    engine
        .scenario_in(run.id, action, about)
        .map_err(|e| e.to_string())
}
