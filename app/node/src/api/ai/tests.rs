use super::*;

#[test]
fn ollamas_own_address_is_the_openai_one_without_its_path() {
    assert_eq!(
        ollama_origin("http://host.docker.internal:11434/v1").as_deref(),
        Some("http://host.docker.internal:11434")
    );
    assert_eq!(
        ollama_origin("http://192.168.1.10:11434/").as_deref(),
        Some("http://192.168.1.10:11434")
    );
    assert_eq!(ollama_origin("not a url"), None);
}

#[test]
fn what_the_model_server_says_about_a_loaded_model_is_read_plainly() {
    let ps = json!({"models": [
        {"name": "qwen2.5:7b", "size": 5_635_173_252u64, "size_vram": 5_635_173_252u64,
         "context_length": 16384, "details": {"parameter_size": "7.6B", "quantization_level": "Q4_K_M"}},
        {"name": "big:70b", "size": 40_000_000_000u64, "size_vram": 10_000_000_000u64,
         "context_length": 8192, "details": {}},
        {"name": "tiny:1b", "size": 1_000_000_000u64, "size_vram": 0, "details": {}}
    ]});
    let qwen = runtime_from_ps(&ps, "qwen2.5:7b").unwrap();
    assert_eq!(qwen["processor"], "graphics card");
    assert_eq!(qwen["memory_gb"], 5.6);
    assert_eq!(qwen["context"], 16384);
    assert_eq!(qwen["size"], "7.6B, Q4_K_M");
    assert_eq!(
        runtime_from_ps(&ps, "big:70b").unwrap()["processor"],
        "graphics card and processor"
    );
    assert_eq!(
        runtime_from_ps(&ps, "tiny:1b").unwrap()["processor"],
        "processor"
    );
    // Not loaded right now: nothing to say, which is not the same as "not running anywhere".
    assert!(runtime_from_ps(&ps, "other:3b").is_none());
    // A name without a tag means ":latest".
    let latest = json!({"models": [{"name": "moondream:latest", "size": 1, "size_vram": 1}]});
    assert!(runtime_from_ps(&latest, "moondream").is_some());
}
