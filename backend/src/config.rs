use anyhow::Result;

fn env_f64(key: &str, fallback: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    /// How long a refresh token lives. Access tokens stay short
    /// (jwt_expiry_hours, default 24h); refresh tokens are long-lived
    /// so users don't need to log in repeatedly while we keep the
    /// access-token blast radius small.
    pub refresh_token_expiry_days: i64,
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
    pub project_data_root: String,
    pub openclaw_price_per_1k_in: f64,
    pub openclaw_price_per_1k_out: f64,
    pub hermes_price_per_1k_in: f64,
    pub hermes_price_per_1k_out: f64,
}

impl Config {
    /// Normalise a provider/agent string into an environment-variable key fragment.
    /// e.g. "gemini-1.5-pro" → "GEMINI_1_5_PRO"
    fn price_env_key(s: &str) -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
            .collect()
    }

    fn env_price_pair(key: &str) -> Option<(f64, f64)> {
        let i = std::env::var(format!("AGENT_PRICE_{key}_PER_1K_INPUT"))
            .ok()?
            .parse::<f64>()
            .ok()?;
        let o = std::env::var(format!("AGENT_PRICE_{key}_PER_1K_OUTPUT"))
            .ok()?
            .parse::<f64>()
            .ok()?;
        Some((i, o))
    }

    /// Return (price_per_1k_input, price_per_1k_output) for the given agent/provider.
    ///
    /// Lookup order:
    /// 1. `AGENT_PRICE_<PROVIDER>_PER_1K_INPUT/OUTPUT` env var when provider is known
    /// 2. Fixed first-party rates for "hermes" and "openclaw"
    /// 3. `AGENT_PRICE_<AGENT>_PER_1K_INPUT/OUTPUT` env var keyed by agent slug
    /// 4. Openclaw rates as final fallback
    pub fn price_for(&self, agent: &str, provider: Option<&str>) -> (f64, f64) {
        if let Some(p) = provider.filter(|p| !p.is_empty()) {
            if let Some(pair) = Self::env_price_pair(&Self::price_env_key(p)) {
                return pair;
            }
        }
        match agent {
            "hermes" => return (self.hermes_price_per_1k_in, self.hermes_price_per_1k_out),
            "openclaw" => return (self.openclaw_price_per_1k_in, self.openclaw_price_per_1k_out),
            _ => {}
        }
        if let Some(pair) = Self::env_price_pair(&Self::price_env_key(agent)) {
            return pair;
        }
        (self.openclaw_price_per_1k_in, self.openclaw_price_per_1k_out)
    }

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
            refresh_token_expiry_days: std::env::var("REFRESH_TOKEN_EXPIRY_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
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
                // 0 = "effectively unlimited" (caps at 50 in orchestrator) —
                // converge via consensus_reached() instead of a hard cap.
                // The default keeps a generous ceiling so deep debates can
                // run without artificially short rounds.
                .unwrap_or(0),
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
            project_data_root: std::env::var("PROJECT_DATA_ROOT")
                .unwrap_or_else(|_| "./data/projects".into())
                .trim_end_matches('/')
                .trim_end_matches('\\')
                .to_string(),
            // Defaults reflect rough OpenAI gpt-4o-mini pricing tier; tune per
            // your actual gateway / model in .env to get accurate Cost panels.
            openclaw_price_per_1k_in: env_f64("OPENCLAW_PRICE_PER_1K_INPUT", 0.15),
            openclaw_price_per_1k_out: env_f64("OPENCLAW_PRICE_PER_1K_OUTPUT", 0.60),
            hermes_price_per_1k_in: env_f64("HERMES_PRICE_PER_1K_INPUT", 0.15),
            hermes_price_per_1k_out: env_f64("HERMES_PRICE_PER_1K_OUTPUT", 0.60),
        })
    }
}
