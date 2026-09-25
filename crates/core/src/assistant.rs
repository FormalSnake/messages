//! Port of packages/core/src/assistant.ts: the CanaryLLM gateway behind the
//! summarize, translate and transcribe buttons. Every call is triggered by a
//! click; nothing here schedules or retries on its own. Cancel by dropping the future.

use std::path::Path;

use crate::model::Millis;

pub const DEFAULT_CANARYLLM_BASE_URL: &str = "https://canaryllm.canarycoders.es";
/// Cheapest Gemini flash-lite model the gateway offers.
pub const DEFAULT_CANARYLLM_MODEL: &str = "gemini/gemini-2.5-flash-lite";
/// Never send more than this many messages, or any attachment bytes, off the machine.
pub const MAX_SUMMARY_MESSAGES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummarizeMessage {
    /// Display name, or "Me" for outgoing. Never a raw address.
    pub sender: String,
    pub text: String,
    pub date: Millis,
    pub from_me: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CanaryLlmError(pub String);

pub struct CanaryLlmClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

impl CanaryLlmClient {
    pub fn new(api_key: String, model: Option<String>, base_url: Option<String>, http: reqwest::Client) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_CANARYLLM_MODEL.to_owned()),
            base_url: base_url.unwrap_or_else(|| DEFAULT_CANARYLLM_BASE_URL.to_owned()),
            http,
        }
    }

    /// `/v1/chat/completions`, 30 s timeout, last MAX_SUMMARY_MESSAGES only, 1 to 3 sentences.
    pub async fn summarize(&self, messages: &[SummarizeMessage]) -> Result<String, CanaryLlmError> {
        let _ = (messages, &self.api_key, &self.model, &self.base_url, &self.http);
        unimplemented!()
    }

    pub async fn translate(&self, text: &str, target_language: &str) -> Result<String, CanaryLlmError> {
        let _ = (text, target_language);
        unimplemented!()
    }

    /// Queued `/api/llm/transcribe` (elevenlabs scribe_v2), polled every 2 s, 120 s total, 90 polls max.
    pub async fn transcribe(&self, audio_path: &Path) -> Result<String, CanaryLlmError> {
        let _ = audio_path;
        unimplemented!()
    }
}
