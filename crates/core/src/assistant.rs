//! The CanaryLLM gateway behind the summarize, translate and transcribe
//! buttons. Every call is triggered by a click; nothing here schedules or
//! retries on its own. Cancel by dropping the future.

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

/// English name of an ISO 639-1 code, for the translate prompt.
fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "eu" => "Basque",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fr" => "French",
        "ga" => "Irish",
        "gl" => "Galician",
        "he" | "iw" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "id" => "Indonesian",
        "is" => "Icelandic",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "ms" => "Malay",
        "nb" | "no" => "Norwegian Bokmål",
        "nl" => "Dutch",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sr" => "Serbian",
        "sv" => "Swedish",
        "th" => "Thai",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "vi" => "Vietnamese",
        "zh" => "Chinese",
        _ => return None,
    })
}

/// The language part of a POSIX or BCP 47 locale: `nl_BE.UTF-8`, `nl-BE`, `en_US@rg=eszzzz` all give the first subtag.
fn locale_language(locale: &str) -> Option<String> {
    let code = locale.split(['_', '-', '.', '@']).next()?.trim().to_lowercase();
    (!code.is_empty() && code != "c" && code != "posix").then_some(code)
}

async fn system_locale() -> Option<String> {
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(language) = std::env::var(name).ok().as_deref().and_then(locale_language) {
            return Some(language);
        }
    }
    // A macOS app started from the Dock has no LANG; the system language list is in the global defaults.
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().await.ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let first = text.split(['(', ')', ',', '\n']).map(|item| item.trim().trim_matches('"')).find(|item| !item.is_empty())?;
        return locale_language(first);
    }
    #[allow(unreachable_code)]
    None
}

fn translation_language_from(configured: Option<&str>, locale: Option<&str>) -> String {
    if let Some(configured) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        return configured.to_owned();
    }
    locale.and_then(language_name).unwrap_or("English").to_owned()
}

/// Target language for the translate button: `config.canaryllm.language`, else the system locale's language written out in English, else English.
pub async fn translation_language(configured: Option<&str>) -> String {
    if configured.is_some_and(|value| !value.trim().is_empty()) {
        return translation_language_from(configured, None);
    }
    translation_language_from(None, system_locale().await.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Response, serve};

    fn one(sender: &str, text: &str) -> Vec<SummarizeMessage> {
        vec![SummarizeMessage { sender: sender.to_owned(), text: text.to_owned(), date: 1, from_me: false }]
    }

    #[tokio::test]
    async fn sends_a_system_and_user_message_and_returns_the_completion_text() {
        let server = serve(|_| Response::json(200, serde_json::json!({ "choices": [{ "message": { "content": "  They agreed on Friday.  " } }] }))).await;
        let client = CanaryLlmClient::new("test-key".into(), None, Some(server.url.clone()), reqwest::Client::new());
        let result = client.summarize(&one("Alice", "Still on for Friday?")).await.unwrap();
        assert_eq!(result, "They agreed on Friday.");
        let requests = server.requests.lock();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path(), "/v1/chat/completions");
        assert_eq!(requests[0].header("authorization"), Some("Bearer test-key"));
        let body = requests[0].json();
        assert_eq!(body["model"], DEFAULT_CANARYLLM_MODEL);
        assert_eq!(body["max_tokens"], 500);
        assert_eq!(body["temperature"], 0.3);
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][1]["content"].as_str().unwrap().contains("Still on for Friday?"));
    }

    #[tokio::test]
    async fn never_sends_more_than_the_last_200_messages_and_uses_the_configured_model() {
        let server = serve(|_| Response::json(200, serde_json::json!({ "choices": [{ "message": { "content": "ok" } }] }))).await;
        let client = CanaryLlmClient::new("k".into(), Some("gemini/gemini-2.5-flash".into()), Some(server.url.clone()), reqwest::Client::new());
        let messages: Vec<SummarizeMessage> = (0..250).map(|i| SummarizeMessage { sender: "Alice".into(), text: format!("message {i}"), date: i, from_me: false }).collect();
        client.summarize(&messages).await.unwrap();
        let body = server.requests.lock()[0].json();
        assert_eq!(body["model"], "gemini/gemini-2.5-flash");
        let transcript = body["messages"][1]["content"].as_str().unwrap().to_owned();
        assert!(!transcript.contains("message 0\n"));
        assert!(transcript.contains("message 249"));
        assert_eq!(transcript.lines().count(), 200);
    }

    #[tokio::test]
    async fn errors_on_an_error_status_and_on_a_completion_with_no_content() {
        let failing = serve(|_| Response::json(500, serde_json::json!({ "error": "boom" }))).await;
        let client = CanaryLlmClient::new("k".into(), None, Some(failing.url.clone()), reqwest::Client::new());
        assert!(client.summarize(&one("A", "hi")).await.unwrap_err().to_string().contains("500"));
        let empty = serve(|_| Response::json(200, serde_json::json!({ "choices": [] }))).await;
        let client = CanaryLlmClient::new("k".into(), None, Some(empty.url.clone()), reqwest::Client::new());
        assert!(client.summarize(&one("A", "hi")).await.unwrap_err().to_string().contains("no content"));
    }

    #[tokio::test]
    async fn sends_the_target_language_in_the_system_prompt_and_the_text_as_the_user_message() {
        let server = serve(|_| Response::json(200, serde_json::json!({ "choices": [{ "message": { "content": "Hola" } }] }))).await;
        let client = CanaryLlmClient::new("k".into(), None, Some(server.url.clone()), reqwest::Client::new());
        assert_eq!(client.translate("Hello", "Spanish").await.unwrap(), "Hola");
        let body = server.requests.lock()[0].json();
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("Spanish"));
        assert_eq!(body["messages"][1]["content"], "Hello");
    }

    #[tokio::test]
    async fn transcribes_through_the_queue_polling_past_a_202() {
        let polls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = polls.clone();
        let server = serve(move |request| match request.path() {
            "/api/llm/transcribe" => Response::json(200, serde_json::json!({ "data": { "queueId": "q1" } })),
            _ if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 => Response::json(202, serde_json::json!({})),
            _ => Response::json(200, serde_json::json!({ "data": { "status": "completed", "result": { "text": "hello there" } } })),
        })
        .await;
        let dir = crate::testing::temp_dir("transcribe");
        let audio = dir.join("note.caf");
        std::fs::write(&audio, b"abc").unwrap();
        let client = CanaryLlmClient::new("k".into(), None, Some(server.url.clone()), reqwest::Client::new());
        assert_eq!(client.transcribe(&audio).await.unwrap(), "hello there");
        let requests = server.requests.lock();
        let submit = requests[0].json();
        assert_eq!(submit["provider"], "elevenlabs");
        assert_eq!(submit["model"], "scribe_v2");
        assert_eq!(submit["mimeType"], "audio/x-caf");
        assert_eq!(submit["audio"], "YWJj");
        assert_eq!(requests[2].json()["queueId"], "q1");
        assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn translation_language_prefers_the_config_then_the_locale_then_english() {
        assert_eq!(translation_language_from(Some(" Dutch "), Some("fr")), "Dutch");
        assert_eq!(translation_language_from(None, locale_language("nl_BE.UTF-8").as_deref()), "Dutch");
        assert_eq!(translation_language_from(None, locale_language("en_US@rg=eszzzz").as_deref()), "English");
        assert_eq!(translation_language_from(Some(""), locale_language("C").as_deref()), "English");
        assert_eq!(translation_language_from(None, Some("xx")), "English");
    }

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
