//! The model declares read-only work; the application binds it to the user's ask.
use crate::{
    ButlerContext, CapabilityRead, CapabilityRunner, CapabilitySpec, CapabilityTool, Decision,
};
use serde_json::{Value, json};

/// Virtual intake tool, handled by orchestration without external side effects.
pub const BEGIN_GOAL: &str = "endora_begin_goal";

/// Maximum concurrent durable requests, enforced during creation.
pub const MAX_ACTIVE_GOALS: usize = 3;

/// Repository rejection that orchestration maps to an actionable conflict.
pub const GOAL_CAPACITY_REACHED: &str = "goal capacity reached";

/// Explicit constraints proposed once and persisted with the original request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GoalCriteria {
    /// Upcoming games for the selected team, evaluated against the actual clock.
    Schedule { team: String, count: usize },
    /// Bounded research; unsupported semantic completion remains partial.
    Research { requirements: Vec<String> },
}

impl GoalCriteria {
    /// Validates the model proposal without granting capability permissions.
    pub fn from_json(value: &Value) -> Result<Self, &'static str> {
        match value["kind"].as_str() {
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
        }
    }
    /// Acknowledges the interpretation so the person can correct it.
    pub fn description(&self) -> String {
        match self {
            Self::Schedule { team, count } => format!("the next {count} {team} games"),
            Self::Research { requirements } => requirements.join("; "),
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
}
