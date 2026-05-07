use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub openclaw_api_url: String,
    pub openclaw_api_key: String,
    pub openclaw_model: String,
    pub hermes_api_url: String,
    pub hermes_api_key: String,
    pub hermes_model: String,
    pub debate_max_rounds: usize,
    pub debate_auto_consensus: bool,
    pub agent_stream_chunk_timeout_secs: u64,
    pub server_host: String,
    pub server_port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/kway_dev".into()),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "kway-dev-secret-change-in-production".into()),
            jwt_expiry_hours: std::env::var("JWT_EXPIRY_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24),
            openclaw_api_url: std::env::var("OPENCLAW_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18789/v1".into()),
            openclaw_api_key: std::env::var("OPENCLAW_API_KEY")
                .or_else(|_| std::env::var("OPENCLAW_GATEWAY_TOKEN"))
                .unwrap_or_default(),
            openclaw_model: std::env::var("OPENCLAW_MODEL").unwrap_or_else(|_| "gpt-5.5".into()),
            hermes_api_url: std::env::var("HERMES_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18790/v1".into()),
            hermes_api_key: std::env::var("HERMES_API_KEY").unwrap_or_default(),
            hermes_model: std::env::var("HERMES_MODEL").unwrap_or_else(|_| "hermes".into()),
            debate_max_rounds: std::env::var("DEBATE_MAX_ROUNDS")
                .ok()
                .and_then(|v| v.parse().ok())
                // Safety cap: prevents debate loops when agents cannot converge.
                .unwrap_or(12),
            debate_auto_consensus: std::env::var("DEBATE_AUTO_CONSENSUS")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(true),
            agent_stream_chunk_timeout_secs: std::env::var("AGENT_STREAM_CHUNK_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300)
                .clamp(30, 900),
            server_host: std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            server_port: std::env::var("SERVER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
        })
    }
}
