//! Smoke test: config.example.toml must parse as a valid Config
//! (regression guard for the bug where partial TOML files failed because
//! nested fields lacked serde(default) attributes).
use contextnest::config::Config;

#[test]
fn config_example_toml_parses() {
    let s = std::fs::read_to_string("config.example.toml").expect("read config.example.toml");
    let c: Config = toml::from_str(&s).expect("parse config.example.toml");
    let emb = c
        .services
        .embedding
        .expect("embedding section present in example");
    assert_eq!(emb.default_model, "qwen3-deepinfra");
    assert!(emb.models.contains_key("qwen3-deepinfra"));
}
