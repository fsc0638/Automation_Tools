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
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            openclaw_api_key: std::env::var("OPENCLAW_API_KEY").unwrap_or_default(),
            openclaw_model: std::env::var("OPENCLAW_MODEL")
                .unwrap_or_else(|_| "gpt-4o".into()),
            hermes_api_url: std::env::var("HERMES_API_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            hermes_api_key: std::env::var("HERMES_API_KEY").unwrap_or_default(),
            hermes_model: std::env::var("HERMES_MODEL")
                .unwrap_or_else(|_| "gpt-4o".into()),
            server_host: std::env::var("SERVER_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()),
            server_port: std::env::var("SERVER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
        })
    }
}
