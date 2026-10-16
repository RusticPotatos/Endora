//! Ignored live acceptance check for ADR 0077.
//!
//! It uses the production SQLite composition, guarded `web_fetch`, the configured
//! model adapter and the real research orchestrator. The source is the reserved
//! documentation domain and the assignment contains no personal data.

use std::sync::Arc;

use endora_application::{
    Clock, ResearchRepository, ResearchRun, ResearchStatus, conduct_research, usecases,
};
use endora_capabilities::{ConfigStore, RegistryRunner, default_capabilities};
use endora_infrastructure::{LlmButler, RandomIdSource, SqliteStore, SystemClock};

#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_MODEL"]
fn a_real_model_delivers_a_saved_report_from_a_guarded_source() {
    let url = std::env::var("ENDORA_MODEL_URL").expect("set ENDORA_MODEL_URL");
    let model = std::env::var("ENDORA_MODEL").expect("set ENDORA_MODEL");
    let store = SqliteStore::open_in_memory().expect("production schema");
    let db = store.db();
    let understanding = endora_understanding::UnderstandingStore::new(db.clone());
    let knowledge = endora_understanding::BrainFolder::temporary().expect("brain");
    let chat = endora_conversation::ChatStore::new(db.clone());
    let config = ConfigStore::new(db.clone());
    let audit = endora_platform::AuditStore::new(db);
    let runner = RegistryRunner::new(Arc::new(default_capabilities()));
    let ids = RandomIdSource;
    let clock = SystemClock;
    let butler = LlmButler::new(url, model);
    let work = store
        .commission_research(
            endora_application::IntentionId::new(1),
            endora_application::MessageId::new(2),
            "live-research-eval-1",
            "Read the supplied page. Explain its intended purpose and give one limitation.",
            r#"["https://example.com/"]"#,
            None,
            clock.now(),
        )
        .expect("commission");
    let context = usecases::butler_context(
        &understanding,
        &knowledge,
        &understanding,
        &config,
        &chat,
        &runner,
        &config,
        &clock,
    )
    .expect("context");

    let result = conduct_research(
        &understanding,
        &store,
        &understanding,
        &runner,
        &butler,
        &audit,
        &ids,
        &clock,
        &context,
        work,
    )
    .expect("research run");
    assert_eq!(result, ResearchRun::Delivered);
    let saved = understanding
        .research(endora_application::IntentionId::new(1))
        .unwrap()
        .unwrap();
    assert_eq!(saved.status(), ResearchStatus::Delivered);
    let markdown = saved.report().expect("saved report").markdown();
    assert!(markdown.contains("## Sources"));
    assert!(markdown.contains("https://example.com/"));
}
