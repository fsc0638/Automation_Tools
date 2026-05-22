use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DataClassification {
    Public = 0,
    Internal = 1,
    Confidential = 2,
    Restricted = 3,
    Secret = 4,
}

impl DataClassification {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "public" => Some(Self::Public),
            "internal" => Some(Self::Internal),
            "confidential" => Some(Self::Confidential),
            "restricted" => Some(Self::Restricted),
            "secret" => Some(Self::Secret),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
            Self::Restricted => "restricted",
            Self::Secret => "secret",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RedactionReport {
    pub redacted_count: usize,
    pub matched_kinds: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanitizedText {
    pub text: String,
    pub report: RedactionReport,
    pub classification: DataClassification,
}

const SECRET_REPLACEMENT: &str = "[REDACTED_SECRET]";

/// Scrub vault-echoed secrets from an AI response before persisting to the
/// `messages` table.
///
/// This is the **last line of defence** if a plaintext secret reached the
/// model via user input (e.g., the user pasted a revealed vault secret into
/// the chat) and the model echoed it back verbatim.
///
/// Applies the full `redact_secrets` ruleset — GitHub PATs, OpenAI keys,
/// AWS keys, JWTs, database URLs, generic `secret=…` assignments, etc.
///
/// Returns `(scrubbed_content, was_modified)`.  When `was_modified` is true
/// the caller should emit a `WARN` log so the event is visible in the
/// security audit trail without needing to store the original text.
pub fn scrub_vault_echo(content: &str) -> (String, bool) {
    let result = redact_secrets(content);
    let modified = result.report.redacted_count > 0;
    (result.text, modified)
}

pub fn redact_secrets(input: &str) -> SanitizedText {
    let mut output = input.to_string();
    let mut report = RedactionReport::default();
    let mut classification = classify_text(input);

    for (kind, pattern, replacement) in secret_patterns() {
        let re = Regex::new(pattern).expect("valid DLP regex");
        let count = re.find_iter(&output).count();
        if count > 0 {
            output = re.replace_all(&output, replacement).to_string();
            report.redacted_count += count;
            if !report.matched_kinds.contains(&kind) {
                report.matched_kinds.push(kind);
            }
            classification = classification.max(DataClassification::Secret);
        }
    }

    SanitizedText {
        text: output,
        report,
        classification,
    }
}

pub fn classify_path(path: &str) -> DataClassification {
    let p = path.replace('\\', "/").to_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);

    if is_blocked_secret_path(&p) {
        return DataClassification::Secret;
    }
    if p.contains("/secrets/")
        || p.contains("/credentials/")
        || p.contains("/private/")
        || name.contains("secret")
        || name.contains("credential")
        || name.ends_with(".key")
        || name.ends_with(".pem")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
    {
        return DataClassification::Restricted;
    }
    if p.contains("contract")
        || p.contains("financial")
        || p.contains("salary")
        || p.contains("customer")
        || p.contains("legal")
    {
        return DataClassification::Confidential;
    }
    if p.ends_with("package-lock.json")
        || p.ends_with("yarn.lock")
        || p.ends_with("pnpm-lock.yaml")
        || p.ends_with("cargo.lock")
        || p.ends_with("go.sum")
    {
        return DataClassification::Internal;
    }
    DataClassification::Public
}

pub fn is_blocked_secret_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    let is_env = name == ".env" || name.starts_with(".env.") || name.ends_with(".env");
    let env_example =
        name.contains("example") || name.contains("sample") || name.ends_with(".template");

    (is_env && !env_example)
        || name == "id_rsa"
        || name == "id_dsa"
        || name == "id_ecdsa"
        || name == "id_ed25519"
        || name.ends_with("_rsa")
        || name.ends_with("_ed25519")
        || name.ends_with(".key")
        || p.contains("/.ssh/")
}

pub fn classify_text(input: &str) -> DataClassification {
    if input.trim().is_empty() {
        return DataClassification::Public;
    }
    let lower = input.to_lowercase();
    if lower.contains("-----begin ") && lower.contains("private key-----") {
        return DataClassification::Secret;
    }
    for (_, pattern, _) in secret_patterns() {
        let re = Regex::new(pattern).expect("valid DLP regex");
        if re.is_match(input) {
            return DataClassification::Secret;
        }
    }
    if lower.contains("confidential")
        || lower.contains("restricted")
        || lower.contains("do not distribute")
    {
        return DataClassification::Confidential;
    }
    DataClassification::Internal
}

fn secret_patterns() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "private_key",
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            "[REDACTED_PRIVATE_KEY]",
        ),
        ("aws_access_key", r"AKIA[0-9A-Z]{16}", SECRET_REPLACEMENT),
        (
            "github_pat",
            r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}",
            SECRET_REPLACEMENT,
        ),
        ("openai_key", r"sk-[A-Za-z0-9_-]{20,}", SECRET_REPLACEMENT),
        (
            "anthropic_key",
            r"sk-ant-[A-Za-z0-9_-]{20,}",
            SECRET_REPLACEMENT,
        ),
        ("google_key", r"AIza[0-9A-Za-z_-]{20,}", SECRET_REPLACEMENT),
        (
            "jwt",
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "[REDACTED_JWT]",
        ),
        (
            "database_url",
            r"(?i)(postgres|postgresql|mysql|mongodb|redis)://[^\s:@/]+:[^\s@/]+@[^\s]+",
            "[REDACTED_DATABASE_URL]",
        ),
        (
            "secret_assignment",
            r#"(?i)\b(password|passwd|api[_-]?key|secret|token|client[_-]?secret|access[_-]?key)\b\s*[:=]\s*['\"]?[^'\"\s]{8,}['\"]?"#,
            "$1=[REDACTED_SECRET]",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_high_signal_secrets() {
        let raw = "OPENAI_API_KEY=sk-testsecretkeyvalue1234567890\nDATABASE_URL=postgres://u:p@localhost/db";
        let sanitized = redact_secrets(raw);
        assert!(sanitized.report.redacted_count >= 2);
        assert!(!sanitized.text.contains("sk-testsecret"));
        assert!(!sanitized.text.contains("postgres://u:p"));
        assert_eq!(sanitized.classification, DataClassification::Secret);
    }

    #[test]
    fn classifies_blocked_secret_paths() {
        assert_eq!(classify_path(".env"), DataClassification::Secret);
        assert_eq!(
            classify_path("config/.env.production"),
            DataClassification::Secret
        );
        assert_eq!(classify_path(".env.example"), DataClassification::Public);
        assert_eq!(
            classify_path("keys/service.pem"),
            DataClassification::Restricted
        );
    }
}
