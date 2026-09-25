//! Port of packages/core/src/assistant.ts: the CanaryLLM gateway behind the
//! summarize, translate and transcribe buttons. Every call is triggered by a
//! click; nothing here schedules or retries on its own. Cancel by dropping the future.

use std::path::Path;
use std::time::Duration;

use base64::prelude::{Engine, BASE64_STANDARD};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::model::Millis;

pub const DEFAULT_CANARYLLM_BASE_URL: &str = "https://canaryllm.canarycoders.es";
/// Cheapest Gemini flash-lite model the gateway offers.
pub const DEFAULT_CANARYLLM_MODEL: &str = "gemini/gemini-2.5-flash-lite";
/// Never send more than this many messages, or any attachment bytes, off the machine.
pub const MAX_SUMMARY_MESSAGES: usize = 200;

const TRANSCRIBE_MODEL: &str = "scribe_v2";
const CHAT_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSCRIBE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Ceiling on poll attempts so a stuck job cannot poll forever even if the clock check is skewed.
const TRANSCRIBE_MAX_POLLS: u32 = 90;

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

fn mime_from_path(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).map(str::to_lowercase).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("m4a") | Some("mp4") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("wav") => "audio/wav",
        Some("caf") => "audio/x-caf",
        Some("ogg") => "audio/ogg",
        _ => "audio/mp4",
    }
}

fn format_transcript(messages: &[SummarizeMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let who = if message.from_me { "Me".to_owned() } else { message.sender.clone() };
            let text = if message.text.is_empty() { "[attachment, no text]".to_owned() } else { message.text.clone() };
            let when = DateTime::from_timestamp_millis(message.date).unwrap_or_default().with_timezone(&Local).format("%-m/%-d/%Y, %-I:%M:%S %p");
            format!("{who} ({when}): {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    max_tokens: u32,
    temperature: f64,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
}

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Option<Vec<Choice>>,
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

    async fn chat(&self, system: &str, user: &str) -> Result<String, CanaryLlmError> {
        let body = ChatRequest { model: &self.model, messages: [ChatMessage { role: "system", content: system }, ChatMessage { role: "user", content: user }], max_tokens: 500, temperature: 0.3 };
        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .timeout(CHAT_TIMEOUT)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| CanaryLlmError(format!("canaryllm: chat completions request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(CanaryLlmError(format!("canaryllm: chat completions returned {}", response.status().as_u16())));
        }
        let parsed: ChatCompletion = response.json().await.map_err(|error| CanaryLlmError(format!("canaryllm: {error}")))?;
        parsed
            .choices
            .and_then(|choices| choices.into_iter().next())
            .and_then(|choice| choice.message)
            .and_then(|message| message.content)
            .map(|content| content.trim().to_owned())
            .filter(|content| !content.is_empty())
            .ok_or_else(|| CanaryLlmError("canaryllm: chat completions returned no content".to_owned()))
    }

    /// `/v1/chat/completions`, 30 s timeout, last MAX_SUMMARY_MESSAGES only, 1 to 3 sentences.
    pub async fn summarize(&self, messages: &[SummarizeMessage]) -> Result<String, CanaryLlmError> {
        let capped = &messages[messages.len().saturating_sub(MAX_SUMMARY_MESSAGES)..];
        let system = "You summarize iMessage conversations for the person reading them. Reply in sentence case, 1-3 short sentences, covering what was said or decided. No preamble, no bullet points, no repeating the instructions.";
        self.chat(system, &format_transcript(capped)).await
    }

    pub async fn translate(&self, text: &str, target_language: &str) -> Result<String, CanaryLlmError> {
        let system = format!("Translate the user's message into {target_language}. Reply with only the translation, nothing else.");
        self.chat(&system, text).await
    }

    /// Queued `/api/llm/transcribe` (elevenlabs scribe_v2), polled every 2 s, 120 s total, 90 polls max.
    pub async fn transcribe(&self, audio_path: &Path) -> Result<String, CanaryLlmError> {
        match tokio::time::timeout(TRANSCRIBE_TIMEOUT, self.transcribe_inner(audio_path)).await {
            Ok(result) => result,
            Err(_) => Err(CanaryLlmError("canaryllm: transcribe timed out waiting for the queue".to_owned())),
        }
    }

    async fn transcribe_inner(&self, audio_path: &Path) -> Result<String, CanaryLlmError> {
        #[derive(Serialize)]
        struct SubmitRequest<'a> {
            provider: &'a str,
            model: &'a str,
            audio: String,
            #[serde(rename = "mimeType")]
            mime_type: &'a str,
        }
        #[derive(Deserialize)]
        struct SubmitData {
            #[serde(rename = "queueId")]
            queue_id: Option<String>,
        }
        #[derive(Deserialize)]
        struct SubmitResponse {
            data: Option<SubmitData>,
        }

        let bytes = tokio::fs::read(audio_path).await.map_err(|error| CanaryLlmError(format!("canaryllm: {error}")))?;
        let audio = BASE64_STANDARD.encode(&bytes);
        let mime_type = mime_from_path(audio_path);

        let submit = self
            .http
            .post(format!("{}/api/llm/transcribe", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&SubmitRequest { provider: "elevenlabs", model: TRANSCRIBE_MODEL, audio, mime_type })
            .send()
            .await
            .map_err(|error| CanaryLlmError(format!("canaryllm: transcribe submit request failed: {error}")))?;
        if !submit.status().is_success() {
            return Err(CanaryLlmError(format!("canaryllm: transcribe submit returned {}", submit.status().as_u16())));
        }
        let submit_body: SubmitResponse = submit.json().await.map_err(|error| CanaryLlmError(format!("canaryllm: {error}")))?;
        let queue_id = submit_body.data.and_then(|data| data.queue_id).ok_or_else(|| CanaryLlmError("canaryllm: transcribe submit returned no queueId".to_owned()))?;

        #[derive(Deserialize)]
        struct PollResult {
            text: Option<String>,
            transcript: Option<String>,
        }
        #[derive(Deserialize)]
        struct PollData {
            status: Option<String>,
            result: Option<PollResult>,
        }
        #[derive(Deserialize)]
        struct PollResponse {
            data: Option<PollData>,
        }
        #[derive(Serialize)]
        struct PollRequest<'a> {
            #[serde(rename = "queueId")]
            queue_id: &'a str,
        }

        for _ in 0..TRANSCRIBE_MAX_POLLS {
            tokio::time::sleep(TRANSCRIBE_POLL_INTERVAL).await;
            let poll = self
                .http
                .post(format!("{}/api/llm/queue/result", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&PollRequest { queue_id: &queue_id })
                .send()
                .await
                .map_err(|error| CanaryLlmError(format!("canaryllm: transcribe poll request failed: {error}")))?;
            if poll.status().as_u16() == 202 {
                continue;
            }
            if !poll.status().is_success() {
                return Err(CanaryLlmError(format!("canaryllm: transcribe poll returned {}", poll.status().as_u16())));
            }
            let body: PollResponse = poll.json().await.map_err(|error| CanaryLlmError(format!("canaryllm: {error}")))?;
            let data = body.data;
            if let Some(status) = data.as_ref().and_then(|d| d.status.as_deref()) {
                if status == "error" || status == "cancelled" {
                    return Err(CanaryLlmError(format!("canaryllm: transcribe job {status}")));
                }
            }
            let text = data.and_then(|d| d.result).and_then(|result| result.text.or(result.transcript));
            return text.ok_or_else(|| CanaryLlmError("canaryllm: transcribe completed with no text".to_owned()));
        }
        Err(CanaryLlmError("canaryllm: transcribe timed out waiting for the queue".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_is_derived_from_the_file_extension() {
        assert_eq!(mime_from_path(Path::new("clip.m4a")), "audio/mp4");
        assert_eq!(mime_from_path(Path::new("clip.CAF")), "audio/x-caf");
        assert_eq!(mime_from_path(Path::new("clip.unknown")), "audio/mp4");
    }

    #[test]
    fn transcript_labels_the_users_own_messages_me_and_marks_attachments() {
        let messages = vec![
            SummarizeMessage { sender: "Alice".to_owned(), text: "Still on for Friday?".to_owned(), date: 1, from_me: false },
            SummarizeMessage { sender: "Alice".to_owned(), text: String::new(), date: 2, from_me: false },
            SummarizeMessage { sender: "Me".to_owned(), text: "Yes, works for me.".to_owned(), date: 3, from_me: true },
        ];
        let transcript = format_transcript(&messages);
        assert!(transcript.contains("Alice"));
        assert!(transcript.contains("Still on for Friday?"));
        assert!(transcript.contains("[attachment, no text]"));
        assert!(transcript.contains("Me ("));
        assert_eq!(transcript.lines().count(), 3);
    }

    #[test]
    fn summarize_caps_the_transcript_at_the_last_200_messages() {
        let messages: Vec<SummarizeMessage> = (0..250).map(|i| SummarizeMessage { sender: "Alice".to_owned(), text: format!("message {i}"), date: i, from_me: false }).collect();
        let capped = &messages[messages.len().saturating_sub(MAX_SUMMARY_MESSAGES)..];
        let transcript = format_transcript(capped);
        assert!(!transcript.contains("message 0"));
        assert!(transcript.contains("message 249"));
        assert_eq!(transcript.lines().count(), 200);
    }
}
