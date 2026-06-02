use deepseek_cursor_proxy_rust::config::AppConfig;
use deepseek_cursor_proxy_rust::http::routes::registered_route_count;
use deepseek_cursor_proxy_rust::protocol::normalize::normalize_reasoning_effort;

#[test]
fn skeleton_exposes_expected_defaults() {
    let config = AppConfig::default();
    assert_eq!(config.bind_address(), "127.0.0.1:9000");
    assert_eq!(registered_route_count(), 6);
    assert_eq!(normalize_reasoning_effort("max"), "max");
}
