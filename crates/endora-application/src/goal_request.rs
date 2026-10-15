//! The model declares read-only work; the application binds it to the user's ask.
use crate::{
    ButlerContext, CapabilityRead, CapabilityRunner, CapabilitySpec, CapabilityTool, Decision,
};
use serde_json::{Value, json};

/// Virtual intake tool, handled by orchestration without external side effects.
pub const BEGIN_GOAL: &str = "endora_begin_goal";

/// Maximum concurrent durable requests, enforced during creation.
pub const MAX_ACTIVE_GOALS: usize = 3;

/// Maximum concurrent goals Endora starts for itself (ADR 0094). One, and counted
/// separately from [`MAX_ACTIVE_GOALS`]: neither lane can exhaust the other's slots.
pub const MAX_ACTIVE_RECORD_GOALS: usize = 1;

/// Repository rejection that orchestration maps to an actionable conflict.
pub const GOAL_CAPACITY_REACHED: &str = "goal capacity reached";

/// The record work that Endora started for itself was derived from (ADR 0094). Each is a fact
/// code can re-read, which is what lets the source, not a model, say the work is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkSource {
    /// Something in the house that stopped answering, as first seen at `since_ms`. The same
    /// device failing again later is a new `since_ms`, so a new situation.
    Trouble {
        /// Which service it is in.
        server: String,
        /// What the service calls it.
        thing: String,
        /// When Endora first saw it this way.
        since_ms: i64,
    },
}

/// Explicit constraints proposed once and persisted with the original request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GoalCriteria {
    /// Upcoming games for the selected team, evaluated against the actual clock.
    Schedule { team: String, count: usize },
    /// Bounded research; unsupported semantic completion remains partial.
    Research { requirements: Vec<String> },
    /// Done when the record it was derived from no longer says so. Only Endora creates
    /// these, from stored records; see [`GoalCriteria::from_json`].
    Resolve { source: WorkSource },
}

impl GoalCriteria {
    /// Validates the model proposal without granting capability permissions.
    ///
    /// A model can declare a schedule or a research goal. It can **not** declare a
    /// [`Resolve`](Self::Resolve) one: that criterion says "done when this record says so",
    /// and only code reading a stored record may claim it (ADR 0094). Rejecting it here, at
    /// the one door the model's words come through, is what keeps a person's goal from
    /// borrowing the lane's authority. Reading back what Endora itself stored is
    /// [`from_stored`](Self::from_stored).
    pub fn from_json(value: &Value) -> Result<Self, &'static str> {
        if value["kind"].as_str() == Some("resolve") {
            return Err("a goal cannot declare that it resolves from a record");
        }
        Self::from_stored(value)
    }

    /// Reads criteria back from a checkpoint Endora wrote, including the ones only Endora
    /// can create.
    pub fn from_stored(value: &Value) -> Result<Self, &'static str> {
        match value["kind"].as_str() {
            Some("resolve") => match value["source"].as_str() {
                Some("trouble") => {
                    let text = |key: &str| {
                        value[key]
                            .as_str()
                            .map(str::trim)
                            .filter(|t| !t.is_empty() && t.chars().count() <= 200)
                            .map(str::to_owned)
                            .ok_or("invalid trouble source")
                    };
                    Ok(Self::Resolve {
                        source: WorkSource::Trouble {
                            server: text("server")?,
                            thing: text("thing")?,
                            since_ms: value["since_ms"].as_i64().ok_or("invalid trouble source")?,
                        },
                    })
                }
                _ => Err("unknown work source"),
            },
            Some("schedule") => {
                let team = value["team"]
                    .as_str()
                    .ok_or("schedule needs a team")?
                    .trim();
                let count = value["count"]
                    .as_u64()
                    .ok_or("schedule needs a requested count")?;
                if team.is_empty()
                    || team.chars().count() > 100
                    || team.chars().any(char::is_control)
                    || !(1..=20).contains(&count)
                {
                    return Err("invalid schedule constraints");
                }
                Ok(Self::Schedule {
                    team: team.to_owned(),
                    count: count as usize,
                })
            }
            Some("research") => {
                let items = value["requirements"]
                    .as_array()
                    .ok_or("research needs requirements")?;
                if items.is_empty() || items.len() > 8 {
                    return Err("research needs 1–8 requirements");
                }
                let mut requirements = Vec::new();
                for item in items {
                    let text = item.as_str().ok_or("invalid research requirement")?.trim();
                    if text.is_empty() || text.chars().count() > 500 {
                        return Err("invalid research requirement");
                    }
                    requirements.push(text.to_owned());
                }
                Ok(Self::Research { requirements })
            }
            _ => Err("unknown goal kind"),
        }
    }
    /// Stable checkpoint representation.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Schedule { team, count } => json!({"kind":"schedule","team":team,"count":count}),
            Self::Research { requirements } => {
                json!({"kind":"research","requirements":requirements})
            }
            Self::Resolve {
                source:
                    WorkSource::Trouble {
                        server,
                        thing,
                        since_ms,
                    },
            } => json!({
                "kind":"resolve","source":"trouble",
                "server":server,"thing":thing,"since_ms":since_ms
            }),
        }
    }
    /// Acknowledges the interpretation so the person can correct it.
    pub fn description(&self) -> String {
        match self {
            Self::Schedule { team, count } => format!("the next {count} {team} games"),
            Self::Research { requirements } => requirements.join("; "),
            Self::Resolve {
                source: WorkSource::Trouble { thing, .. },
            } => format!("find out why {thing} is not answering, or that it is again"),
        }
    }
}

/// Research tools must be reached through durable work, not an incidental turn.
pub fn is_research_tool(id: &str) -> bool {
    matches!(id, "sports_schedule" | "web_fetch" | "web_search")
        || id.ends_with(".brave_web_search")
        || id.ends_with(".brave_news_search")
}

/// Offers one native declaration tool alongside the existing conversation/actions.
pub fn intake_context(context: &ButlerContext) -> ButlerContext {
    let mut result = context.clone();
    result.tools.retain(|tool| !is_research_tool(&tool.id));
    result.deferred.retain(|tool| !is_research_tool(&tool.id));
    result.tools.push(CapabilityTool {id:BEGIN_GOAL.to_owned(),description:"For a request requiring a schedule lookup or web research, declare a read-only goal now. Interpret the user's actual team, count or research requirements. Execution will use real tools, recover from failed sources, and post the result. Ordinary conversation needs no goal. Ask one question if essential information is missing.".to_owned(),input_schema:Some(json!({"type":"object","properties":{"kind":{"type":"string","enum":["schedule","research"]},"team":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":20},"requirements":{"type":"array","items":{"type":"string"}}},"required":["kind"]}).to_string())});
    result
        .capabilities
        .retain(|text| !is_research_tool(text.split_whitespace().next().unwrap_or("")));
    result
}

/// Keeps declared research out of the incidental chat path, including direct calls.
pub struct IntakeRunner<'a>(pub &'a dyn CapabilityRunner);
impl CapabilityRunner for IntakeRunner<'_> {
    fn available(&self) -> Vec<CapabilitySpec> {
        self.0
            .available()
            .into_iter()
            .filter(|s| !is_research_tool(&s.id))
            .collect()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        self.run_observed(id, input).map(|r| r.text)
    }
    fn run_observed(&self, id: &str, input: &str) -> Result<CapabilityRead, String> {
        if is_research_tool(id) {
            return Err("research must first be declared as a durable goal".to_owned());
        }
        self.0.run_observed(id, input)
    }
    fn about_the_person(&self) -> Vec<String> {
        self.0.about_the_person()
    }
    fn current_states(&self) -> Vec<(String, String)> {
        self.0.current_states()
    }
    fn decision(&self, id: &str) -> Option<Decision> {
        if is_research_tool(id) {
            return Some(Decision::Block);
        }
        self.0.decision(id)
    }
    fn verifier(&self, id: &str) -> Option<String> {
        self.0.verifier(id)
    }
    fn read_back_input(&self, id: &str, input: &str) -> String {
        self.0.read_back_input(id, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn goal_constraints_are_explicit_and_bounded() {
        let criteria =
            GoalCriteria::from_json(&json!({"kind":"schedule","team":"Example Team","count":4}))
                .unwrap();
        assert_eq!(
            GoalCriteria::from_json(&criteria.to_json()).unwrap(),
            criteria
        );
        for input in [
            json!({"kind":"schedule","team":"","count":3}),
            json!({"kind":"schedule","team":"Example Team","count":0}),
            json!({"kind":"schedule","team":"Example Team","count":21}),
            json!({"kind":"research","requirements":[]}),
            json!({"kind":"send_message"}),
        ] {
            assert!(GoalCriteria::from_json(&input).is_err());
        }
    }

    fn trouble() -> GoalCriteria {
        GoalCriteria::Resolve {
            source: WorkSource::Trouble {
                server: "home".to_owned(),
                thing: "sensor.garage_door".to_owned(),
                since_ms: 1_000,
            },
        }
    }

    #[test]
    fn a_model_cannot_declare_that_a_goal_resolves_from_a_record() {
        // The model's words come through `from_json`. If it accepted `resolve`, a request
        // the person made could carry the authority of work Endora derived from a record,
        // and the model would be choosing which records count.
        let declared = trouble().to_json();
        assert_eq!(declared["kind"], "resolve");
        assert!(GoalCriteria::from_json(&declared).is_err());
    }

    #[test]
    fn what_endora_stored_reads_back_exactly() {
        let criteria = trouble();
        assert_eq!(
            GoalCriteria::from_stored(&criteria.to_json()).unwrap(),
            criteria
        );
        // Ordinary criteria read back the same way through both doors.
        let schedule = GoalCriteria::Schedule {
            team: "Example Team".to_owned(),
            count: 3,
        };
        assert_eq!(
            GoalCriteria::from_stored(&schedule.to_json()).unwrap(),
            schedule
        );
    }

    #[test]
    fn a_stored_source_that_cannot_be_read_is_refused() {
        for input in [
            json!({"kind":"resolve"}),
            json!({"kind":"resolve","source":"something-newer"}),
            json!({"kind":"resolve","source":"trouble","server":"home","thing":"","since_ms":1}),
            json!({"kind":"resolve","source":"trouble","server":"home","thing":"x"}),
            json!({"kind":"resolve","source":"trouble","thing":"x","since_ms":1}),
        ] {
            assert!(GoalCriteria::from_stored(&input).is_err(), "{input}");
        }
    }
}
