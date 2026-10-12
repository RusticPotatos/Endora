//! Ignored live evaluation: given a question, which tool does a model reach for?
//!
//! This asks one question — can a small local model *choose* well among the skills a
//! real node offers it? — and measures it, rather than arguing it. For each model it
//! offers the same catalogue through the same adapter the chat uses, asks a fixed set of
//! questions, and records the **first tool the model asks to call**. Nothing is executed:
//! the first choice is the decision under test.
//!
//! The catalogue is the real built-in skills plus stand-ins for a node's MCP tools, sized
//! and named like a real one (a few dozen tools from each of a handful of servers). The
//! stand-ins' *descriptions* are approximations, because a node reads the real ones from
//! its servers; names carry most of the signal, but treat absolute numbers with care and
//! the comparison between models and between catalogues as the finding.
//!
//! Two conditions per model: **full** (everything on the table) and **shortlisted**
//! (the same question with tools for the house and unrelated servers removed unless the
//! question is about the house — what code could do before the model chooses).
//!
//! Needs a live endpoint: set ENDORA_MODEL_URL and ENDORA_EVAL_MODELS (comma separated).
//! No personal data: places and teams are the placeholder ones from AGENTS.md.

use endora_application::{Butler, ButlerContext, CapabilityTool, TurnMessage};
use endora_capabilities::default_capabilities;
use endora_infrastructure::LlmButler;

/// What the question is really about, which decides what a good answer looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
enum About {
    /// Needs fresh information from outside the house.
    TheWorld,
    /// Is about, or acts on, the house.
    TheHouse,
    /// Needs no tool at all.
    Nothing,
}

struct Case {
    question: &'static str,
    about: About,
    /// First calls that count as a good choice. Empty means "no call" is the good choice.
    good: &'static [&'static str],
}

const WEB: &[&str] = &[
    "brave-search.brave_web_search",
    "brave-search.brave_local_search",
    "brave-search.brave_news_search",
    "brave-search.brave_place_search",
    "brave-search.brave_llm_context",
];

fn cases() -> Vec<Case> {
    vec![
        Case {
            // The question from a real conversation, with the placeholder city.
            question: "what watch events are in new york this month?",
            about: About::TheWorld,
            good: WEB,
        },
        Case {
            question: "can you look online for the best watch repair shops in new york?",
            about: About::TheWorld,
            good: WEB,
        },
        Case {
            question: "are there any concerts in new york this weekend?",
            about: About::TheWorld,
            good: &[
                "ticketed_events",
                "ticketmaster.event_search",
                "ticketmaster.attraction_search",
                "brave-search.brave_web_search",
                "brave-search.brave_local_search",
                "brave-search.brave_news_search",
            ],
        },
        Case {
            question: "when do the yankees play next?",
            about: About::TheWorld,
            good: &[
                "ticketed_events",
                "sports_schedule",
                "ticketmaster.event_search",
                "ticketmaster.attraction_search",
                "brave-search.brave_web_search",
                "brave-search.brave_news_search",
            ],
        },
        Case {
            question: "what's the weather like in new york on saturday?",
            about: About::TheWorld,
            good: &["weather"],
        },
        Case {
            question: "what is on the city council agenda this month?",
            about: About::TheWorld,
            good: &["city_meetings"],
        },
        Case {
            question: "turn on the kitchen light",
            about: About::TheHouse,
            good: &["home-assistant.HassTurnOn", "home-assistant.HassLightSet"],
        },
        Case {
            question: "what is the capital of france?",
            about: About::Nothing,
            good: &[],
        },
    ]
}

/// The tool names a real node enabled from three MCP servers: a search server, a
/// home-automation server and an events/entity-research server. Names only; the
/// descriptions below are approximations.
const MCP_TOOL_IDS: &[&str] = &[
    "brave-search.brave_image_search",
    "brave-search.brave_llm_context",
    "brave-search.brave_local_search",
    "brave-search.brave_news_search",
    "brave-search.brave_place_search",
    "brave-search.brave_summarizer",
    "brave-search.brave_video_search",
    "brave-search.brave_web_search",
    "home-assistant.GetDateTime",
    "home-assistant.GetLiveContext",
    "home-assistant.HassBroadcast",
    "home-assistant.HassCancelAllTimers",
    "home-assistant.HassLightSet",
    "home-assistant.HassListAddItem",
    "home-assistant.HassListCompleteItem",
    "home-assistant.HassListRemoveItem",
    "home-assistant.HassMediaNext",
    "home-assistant.HassMediaPause",
    "home-assistant.HassMediaPlayerMute",
    "home-assistant.HassMediaPlayerUnmute",
    "home-assistant.HassMediaPrevious",
    "home-assistant.HassMediaSearchAndPlay",
    "home-assistant.HassMediaUnpause",
    "home-assistant.HassSetVolume",
    "home-assistant.HassSetVolumeRelative",
    "home-assistant.HassTurnOff",
    "home-assistant.HassTurnOn",
    "home-assistant.assist_satellite__HassBroadcast",
    "home-assistant.homeassistant__GetLiveContext",
    "home-assistant.intent__HassCancelAllTimers",
    "home-assistant.intent__HassTurnOff",
    "home-assistant.intent__HassTurnOn",
    "home-assistant.light__HassLightSet",
    "home-assistant.llm__GetDateTime",
    "home-assistant.media_player__HassMediaNext",
    "home-assistant.media_player__HassMediaPause",
    "home-assistant.media_player__HassMediaPlayerMute",
    "home-assistant.media_player__HassMediaPlayerUnmute",
    "home-assistant.media_player__HassMediaPrevious",
    "home-assistant.media_player__HassMediaSearchAndPlay",
    "home-assistant.media_player__HassMediaUnpause",
    "home-assistant.media_player__HassSetVolume",
    "home-assistant.media_player__HassSetVolumeRelative",
    "home-assistant.todo__HassListAddItem",
    "home-assistant.todo__HassListCompleteItem",
    "home-assistant.todo__HassListRemoveItem",
    "home-assistant.todo__get_items",
    "home-assistant.todo_get_items",
    "ticketmaster.ai_visibility_check",
    "ticketmaster.ask_pipeworx",
    "ticketmaster.ask_pipeworx_beta",
    "ticketmaster.ask_pipeworx_grounded",
    "ticketmaster.attraction",
    "ticketmaster.attraction_search",
    "ticketmaster.bet_research",
    "ticketmaster.classification",
    "ticketmaster.classifications",
    "ticketmaster.compare_entities",
    "ticketmaster.deep_research",
    "ticketmaster.discover_tools",
    "ticketmaster.entity_profile",
    "ticketmaster.event",
    "ticketmaster.event_images",
    "ticketmaster.event_search",
    "ticketmaster.forget",
    "ticketmaster.generate_llms_txt",
    "ticketmaster.list_subscriptions",
    "ticketmaster.pipeworx_feedback",
    "ticketmaster.pipeworx_trending",
    "ticketmaster.polymarket_arbitrage",
    "ticketmaster.polymarket_edge_tracker",
    "ticketmaster.polymarket_edges",
    "ticketmaster.polymarket_fill_risk",
    "ticketmaster.polymarket_kalshi_spread",
    "ticketmaster.recall",
    "ticketmaster.recent_alerts",
    "ticketmaster.recent_changes",
    "ticketmaster.remember",
    "ticketmaster.resolve_entity",
    "ticketmaster.scan_competitor_ai_presence",
    "ticketmaster.scan_dependency",
    "ticketmaster.search_within",
    "ticketmaster.subscribe",
    "ticketmaster.suggest",
    "ticketmaster.suggest_questions",
    "ticketmaster.unsubscribe",
    "ticketmaster.validate_claim",
    "ticketmaster.venue",
    "ticketmaster.venue_search",
];

fn brave_description(tail: &str) -> Option<&'static str> {
    Some(match tail {
        "brave_web_search" => {
            "Performs web searches using the Brave Search API, ideal for general queries, \
             news, articles, and online content. Use this for broad information gathering, \
             recent events, or when you need diverse web sources."
        }
        "brave_local_search" => {
            "Searches for local businesses and places using Brave's Local Search API. Best \
             for queries about physical locations, businesses, restaurants and services."
        }
        "brave_news_search" => {
            "Searches for news articles using the Brave News Search API. Ideal for current \
             events, breaking news, and recent developments."
        }
        "brave_place_search" => "Searches for places and points of interest near a location.",
        "brave_image_search" => "Searches the web for images.",
        "brave_video_search" => "Searches the web for videos.",
        "brave_summarizer" => "Summarises the results of a previous web search.",
        "brave_llm_context" => "Fetches web content formatted as context for a language model.",
        _ => return None,
    })
}

/// One stand-in tool: the real id, a description made from it, a plausible schema.
fn stand_in(id: &str) -> CapabilityTool {
    let tail = id.split_once('.').map_or(id, |(_, rest)| rest);
    let server = id.split_once('.').map_or("", |(server, _)| server);
    let description = brave_description(tail).map_or_else(
        || format!("{}: {}", server, tail.replace('_', " ")),
        str::to_owned,
    );
    let schema = if server == "brave-search" {
        r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"count":{"type":"integer"}},"required":["query"]}"#
    } else {
        r#"{"type":"object","properties":{"name":{"type":"string"}}}"#
    };
    CapabilityTool {
        id: id.to_owned(),
        description,
        input_schema: Some(schema.to_owned()),
    }
}

/// Every built-in skill, from its own description (the one that needs a search key a real
/// node does not have is left out), plus the MCP stand-ins. Read from the skills directly
/// rather than from a registry, which would drop every skill that is not configured *in
/// this process* — a real node has them configured.
fn catalogue() -> Vec<CapabilityTool> {
    let mut tools: Vec<CapabilityTool> = default_capabilities()
        .iter()
        .map(|skill| skill.info())
        .filter(|info| info.id != "web_search")
        .map(|info| CapabilityTool {
            id: info.id.to_owned(),
            description: info.description.to_owned(),
            input_schema: info.input_schema.map(str::to_owned),
        })
        .collect();
    tools.extend(MCP_TOOL_IDS.iter().map(|id| stand_in(id)));
    tools
}

/// What code could do before the model chooses: keep the built-ins and the search server,
/// and drop the house's tools and the unrelated server unless the question is about the
/// house.
fn shortlisted(all: &[CapabilityTool], about: About) -> Vec<CapabilityTool> {
    all.iter()
        .filter(|tool| {
            let mcp_noise = tool.id.starts_with("ticketmaster.");
            let house = tool.id.starts_with("home-assistant.");
            match about {
                About::TheHouse => !mcp_noise,
                _ => !mcp_noise && !house,
            }
        })
        .cloned()
        .collect()
}

#[test]
#[ignore = "prints the catalogue the evaluation uses"]
fn print_the_catalogue() {
    let all = catalogue();
    eprintln!("{} tools in the full catalogue:", all.len());
    for tool in all.iter().filter(|t| !t.id.contains('.')) {
        eprintln!(
            "  built-in {:<24} {}",
            tool.id,
            tool.description.chars().take(70).collect::<String>()
        );
    }
}

/// The first tool the model asks to call, or `None` when it answers without one.
fn first_choice(
    butler: &LlmButler,
    tools: Vec<CapabilityTool>,
    question: &str,
) -> Result<Option<String>, String> {
    let context = ButlerContext {
        capabilities: tools
            .iter()
            .map(|t| format!("{}: {}", t.id, t.description))
            .collect(),
        tools,
        where_they_are: "New York".to_owned(),
        ..ButlerContext::default()
    };
    let reply = butler
        .take_turn(&[TurnMessage::User(question.to_owned())], &[], &context)
        .map_err(|e| e.to_string())?;
    Ok(reply.tool_calls.first().map(|call| call.capability.clone()))
}

#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_EVAL_MODELS"]
fn which_tool_does_each_model_reach_for() {
    let url = std::env::var("ENDORA_MODEL_URL").expect("set ENDORA_MODEL_URL");
    let models = std::env::var("ENDORA_EVAL_MODELS").expect("set ENDORA_EVAL_MODELS");
    let repeats: usize = std::env::var("ENDORA_EVAL_REPEATS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let all = catalogue();
    eprintln!(
        "catalogue: {} tools; {repeats} repeat(s) per question\n",
        all.len()
    );

    for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let butler = LlmButler::new(url.clone(), model.to_owned());
        let mut totals = [(0usize, 0usize); 2]; // (good, asked) for full, shortlisted
        let mut world = [(0usize, 0usize); 2];
        eprintln!("== {model}");
        for case in cases() {
            let mut line = format!("  {:<62}", case.question);
            for (condition, label) in ["full", "short"].iter().enumerate() {
                let mut picks: Vec<String> = Vec::new();
                let mut good = 0;
                for _ in 0..repeats {
                    let tools = if condition == 0 {
                        all.clone()
                    } else {
                        shortlisted(&all, case.about)
                    };
                    let choice = first_choice(&butler, tools, case.question);
                    let pick = match &choice {
                        Ok(Some(id)) => id.clone(),
                        Ok(None) => "(none)".to_owned(),
                        Err(e) => format!("(error: {e})"),
                    };
                    let ok = match &choice {
                        Ok(Some(id)) => case.good.contains(&id.as_str()),
                        Ok(None) => case.good.is_empty(),
                        Err(_) => false,
                    };
                    good += usize::from(ok);
                    picks.push(pick);
                }
                totals[condition].0 += good;
                totals[condition].1 += repeats;
                if case.about == About::TheWorld {
                    world[condition].0 += good;
                    world[condition].1 += repeats;
                }
                picks.sort();
                picks.dedup();
                line.push_str(&format!(" {label}: {good}/{repeats} {picks:?}"));
            }
            eprintln!("{line}");
        }
        eprintln!(
            "  SUMMARY {model}: full {}/{} (world questions {}/{}), shortlisted {}/{} (world {}/{})\n",
            totals[0].0,
            totals[0].1,
            world[0].0,
            world[0].1,
            totals[1].0,
            totals[1].1,
            world[1].0,
            world[1].1
        );
    }
}

/// What the model does next, given a conversation so far: the first tool it asks to call
/// (if any) and whatever prose it wrote.
fn next_step(
    butler: &LlmButler,
    tools: Vec<CapabilityTool>,
    conversation: &[TurnMessage],
) -> Result<(Option<String>, String), String> {
    let context = ButlerContext {
        capabilities: tools
            .iter()
            .map(|t| format!("{}: {}", t.id, t.description))
            .collect(),
        tools,
        where_they_are: "New York".to_owned(),
        ..ButlerContext::default()
    };
    let reply = butler
        .take_turn(conversation, &[], &context)
        .map_err(|e| e.to_string())?;
    Ok((
        reply.tool_calls.first().map(|call| call.capability.clone()),
        reply.text,
    ))
}

/// The case from a real conversation, one step later: the first tool came back **empty**.
/// A good assistant does not stop at "nothing found" when it has a tool that could answer
/// the question; a poor one reports the empty result as the answer. This measures which.
#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_EVAL_MODELS"]
fn after_an_empty_result_does_the_model_look_elsewhere() {
    let url = std::env::var("ENDORA_MODEL_URL").expect("set ENDORA_MODEL_URL");
    let models = std::env::var("ENDORA_EVAL_MODELS").expect("set ENDORA_EVAL_MODELS");
    let repeats: usize = std::env::var("ENDORA_EVAL_REPEATS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let all = catalogue();
    let question = "what watch events are in new york this month?";
    let conversation_for = |empty_result: &str| {
        vec![
            TurnMessage::User(question.to_owned()),
            TurnMessage::Assistant {
                text: String::new(),
                tool_calls: vec![endora_application::ToolCall {
                    id: "call_1".to_owned(),
                    capability: "ticketed_events".to_owned(),
                    input_json: r#"{"what":"watch events","city":"New York"}"#.to_owned(),
                }],
            },
            TurnMessage::ToolResult {
                call_id: "call_1".to_owned(),
                content: empty_result.to_owned(),
            },
        ]
    };

    for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let butler = LlmButler::new(url.clone(), model.to_owned());
        eprintln!("== {model}: after `ticketed_events` returned nothing");
        for (wording, empty_result) in [
            ("as it reads now ", NOTHING_TICKETED_NOW),
            ("pointing elsewhere", NOTHING_TICKETED_POINTING_ELSEWHERE),
        ] {
            let conversation = conversation_for(empty_result);
            for (label, tools) in [
                ("full ", all.clone()),
                ("short", shortlisted(&all, About::TheWorld)),
            ] {
                let (mut elsewhere, mut repeated, mut gave_up) = (0, 0, 0);
                let mut said: Vec<String> = Vec::new();
                for _ in 0..repeats {
                    match next_step(&butler, tools.clone(), &conversation) {
                        Ok((Some(id), _)) if WEB.contains(&id.as_str()) => elsewhere += 1,
                        Ok((Some(id), _)) if id == "ticketed_events" => repeated += 1,
                        Ok((Some(id), _)) => {
                            said.push(format!("called {id}"));
                            repeated += 1;
                        }
                        Ok((None, text)) => {
                            gave_up += 1;
                            said.push(text.chars().take(80).collect());
                        }
                        Err(e) => said.push(format!("error: {e}")),
                    }
                }
                said.sort();
                said.dedup();
                eprintln!(
                    "  [{wording}] {label}: looked at the web {elsewhere}/{repeats}, another \
                     ticket/other tool {repeated}/{repeats}, answered without looking \
                     {gave_up}/{repeats} {said:?}"
                );
            }
        }
        eprintln!();
    }
}

/// What the skill says today when it finds nothing.
const NOTHING_TICKETED_NOW: &str = "Nothing ticketed coming up for that.";

/// The same, saying what it does not cover and where to look instead.
const NOTHING_TICKETED_POINTING_ELSEWHERE: &str = "Nothing ticketed coming up for that. This \
    only covers ticketed events — concerts, sport, shows and theatre — so it says nothing \
    about other kinds of event. If the question is about something else, search the web.";
