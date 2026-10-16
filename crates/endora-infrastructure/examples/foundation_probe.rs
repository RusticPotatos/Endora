//! Local-model benchmark through Endora's production chat use case and SQLite.
//! Fixture tools replace external services, never the model or orchestration.
use endora_application::{
    ButlerContext, CapabilityRunner, CapabilitySpec, Clock, Reversibility, Timestamp, usecases,
};
use endora_infrastructure::{LlmButler, RandomIdSource, SqliteStore};
use serde_json::{Value, json};
use std::{env, fs, sync::Mutex, time::Instant};

struct FixtureRunner {
    specs: Vec<CapabilitySpec>,
    task: Value,
    calls: Mutex<Vec<Value>>,
}
impl CapabilityRunner for FixtureRunner {
    fn available(&self) -> Vec<CapabilitySpec> {
        self.specs.clone()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        let args: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        self.calls
            .lock()
            .unwrap()
            .push(json!({"tool":id,"input":args}));
        match id {
            "sports_schedule" => Err("Source refused this read: HTTP 403".to_owned()),
            "web_search" => Ok(self.task["search"].to_string()),
            "web_fetch" => self.task["pages"][args["url"].as_str().unwrap_or("")]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Fixture source unavailable".to_owned()),
            _ => Err("Tool not permitted by benchmark".to_owned()),
        }
    }
}
struct FixtureClock;
impl Clock for FixtureClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(1_914_969_600_000)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture: Value =
        serde_json::from_str(&fs::read_to_string(env::var("ENDORA_FIXTURE_FILE")?)?)?;
    let case = env::var("ENDORA_FIXTURE_CASE")?;
    let task = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == case)
        .ok_or("unknown fixture")?
        .clone();
    let specs = fixture["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| CapabilitySpec {
            id: t["name"].as_str().unwrap().to_owned(),
            description: t["description"].as_str().unwrap().to_owned(),
            configured: true,
            autonomous: true,
            third_party: true,
            wants_place: false,
            reversibility: Reversibility::Observe,
            input_schema: Some(t["parameters"].to_string()),
        })
        .collect();
    let runner = FixtureRunner {
        specs,
        task: task.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let store = SqliteStore::open_in_memory()?;
    let db = store.db();
    let understanding = endora_understanding::UnderstandingStore::new(db.clone());
    let knowledge = endora_understanding::BrainFolder::temporary()?;
    let chat = endora_conversation::ChatStore::new(db.clone());
    let config = endora_capabilities::ConfigStore::new(db.clone());
    let audit = endora_platform::AuditStore::new(db);
    let model = env::var("ENDORA_MODEL")?;
    let url = env::var("ENDORA_MODEL_URL")?;
    let butler = LlmButler::new(url, model.clone());
    let context: ButlerContext = usecases::butler_context(
        &understanding,
        &knowledge,
        &understanding,
        &config,
        &chat,
        &runner,
        &config,
        &FixtureClock,
    )?;
    let start = Instant::now();
    let mut events = Vec::new();
    let mut tokens = Vec::new();
    let (answer, _) = usecases::send_to_butler_streaming(
        &chat,
        &knowledge,
        &understanding,
        &understanding,
        &runner,
        &butler,
        &audit,
        None,
        &RandomIdSource,
        &FixtureClock,
        &context,
        task["prompt"].as_str().unwrap(),
        &mut |text| tokens.push(json!({"ms":start.elapsed().as_millis(),"text":text})),
        &mut |step| {
            events.push(json!({"ms":start.elapsed().as_millis(),"tool":step.skill,"status":step.status.as_str()}))
        },
        &mut Vec::new(),
    )?;
    println!(
        "{}",
        json!({"runtime":"endora","case":case,"model":model,
        "elapsed_ms":start.elapsed().as_millis(),"answer":answer.text(),
        "calls":runner.calls.into_inner().unwrap(),"tokens":tokens,"events":events,
        "review_status":"unreviewed"})
    );
    Ok(())
}
