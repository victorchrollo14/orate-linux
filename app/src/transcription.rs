use base64::Engine;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Prefs, Provider};

const ORATE_CLOUD_BASE: &str = "https://orate-api.lavisht22.workers.dev";
// Only used for Google AI Studio / Vertex AI; Orate Cloud picks its own model
// server-side. 3.5-flash-lite answers in ~2s where 3.1-flash-lite took 9–28s.
pub const MODEL: &str = "gemini-3.5-flash-lite";

const SYSTEM_PROMPT: &str = "You are Orate, an intelligent speech-to-text assistant. Your job is to transcribe spoken audio and produce clean, polished text ready to be inserted directly into whatever the user is typing.\n\nCore rules:\n- Transcribe the spoken content accurately, preserving the speaker's intended meaning.\n- Clean up speech disfluencies: remove filler words (um, uh, like, you know), false starts, and repeated words \u{2014} unless they are clearly intentional for emphasis.\n- Fix grammar and punctuation naturally. Add proper capitalization, periods, commas, and other punctuation as appropriate for written text.\n- Do NOT add any preamble, commentary, labels, or formatting beyond the transcription itself. Output ONLY the final clean text.\n- Do NOT wrap the output in quotes or add \"Transcription:\" or similar prefixes.\n- If the speaker dictates punctuation explicitly (e.g. says \"period\", \"comma\", \"new line\", \"question mark\"), convert those to the actual punctuation characters.\n- CRITICAL: If the audio contains no spoken words (silence, background noise, breathing, typing, or other non-speech sounds), you MUST output an empty string. Do not generate any text whatsoever \u{2014} not even from vocabulary hints or custom instructions. Only transcribe actual spoken words.\n- Preserve the speaker's tone and intent: if they are writing a casual message, keep it casual. If formal, keep it formal.\n- For numbers, use digits for quantities and measurements (e.g. \"5 minutes\", \"200 users\") and words for conversational usage (e.g. \"a couple of things\").\n- If the speaker is dictating a list (e.g. \"first... second... third...\" or \"number one... number two...\" or \"bullet point...\"), format the output as a properly structured list with line breaks and markers (1. 2. 3. or - bullets) as appropriate.";

#[derive(Debug)]
pub struct TranscriptionResult {
    pub transcript: String,
    pub latency_ms: u128,
    /// Orate Cloud bills in words; the Gemini providers don't report these.
    pub words_used: Option<u64>,
    pub words_remaining: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error("missing {0} API key \u{2014} open Orate settings to add it")]
    MissingApiKey(&'static str),
    #[error("Vertex AI needs a project ID \u{2014} open Orate settings to add it")]
    MissingVertexConfig,
    #[error("insufficient balance \u{2014} please recharge your Orate Cloud key")]
    InsufficientBalance,
    #[error("API error {0}: {1}")]
    Api(u16, String),
    #[error("network: {0}")]
    Network(String),
    #[error("parse: {0}")]
    Parse(String),
}

impl From<reqwest::Error> for TranscriptionError {
    fn from(e: reqwest::Error) -> Self {
        TranscriptionError::Network(e.to_string())
    }
}

#[derive(Serialize)]
struct OrateRequest {
    audio: String,
    system_prompt: String,
}

#[derive(Deserialize)]
struct OrateResponse {
    text: Option<String>,
    error: Option<String>,
    words_used: Option<u64>,
    words_remaining: Option<u64>,
}

pub async fn transcribe(
    audio: &[u8],
    api_key: &str,
    prefs: &Prefs,
) -> Result<TranscriptionResult, TranscriptionError> {
    if api_key.is_empty() {
        return Err(TranscriptionError::MissingApiKey(prefs.provider.display_name()));
    }

    let system_prompt = build_system_prompt(Some(&prefs.custom_instructions), &prefs.vocabulary);
    let audio_b64 = base64::engine::general_purpose::STANDARD.encode(audio);

    match prefs.provider {
        Provider::OrateCloud => transcribe_orate_cloud(audio_b64, system_prompt, api_key).await,
        Provider::GoogleAI | Provider::VertexAI => {
            transcribe_gemini(audio_b64, system_prompt, api_key, prefs).await
        }
    }
}

async fn transcribe_orate_cloud(
    audio_b64: String,
    system_prompt: String,
    api_key: &str,
) -> Result<TranscriptionResult, TranscriptionError> {
    let body = OrateRequest {
        audio: audio_b64,
        system_prompt,
    };

    let url = format!("{ORATE_CLOUD_BASE}/transcribe");
    debug!(
        "POST {url} (base64_chars={}, prompt_chars={})",
        body.audio.len(),
        body.system_prompt.len()
    );

    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    let response = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let latency_ms = start.elapsed().as_millis();

    let status = response.status();
    // Read as text first so we can include the raw body in error logs when
    // the worker returns HTML (401 Cloudflare page, etc.) or malformed JSON.
    let body_text = response
        .text()
        .await
        .map_err(|e| TranscriptionError::Network(format!("reading body: {e}")))?;
    debug!(
        "response: status={} latency={}ms body_chars={}",
        status,
        latency_ms,
        body_text.len()
    );

    let parsed: OrateResponse = match serde_json::from_str(&body_text) {
        Ok(p) => p,
        Err(e) => {
            error!(
                "transcription response was not JSON (status={}): {}\n--- body ---\n{}\n--- end ---",
                status,
                e,
                truncate(&body_text, 1024)
            );
            return Err(TranscriptionError::Parse(format!(
                "status {} body: {}",
                status,
                truncate(&body_text, 200)
            )));
        }
    };

    if let Some(err) = parsed.error {
        warn!("API returned error field: {err} (status={status})");
        if err == "insufficient_balance" {
            return Err(TranscriptionError::InsufficientBalance);
        }
        return Err(TranscriptionError::Api(status.as_u16(), err));
    }

    if !status.is_success() {
        return Err(TranscriptionError::Api(
            status.as_u16(),
            "non-2xx response".to_string(),
        ));
    }

    let text = parsed
        .text
        .ok_or_else(|| TranscriptionError::Parse("missing 'text' field".to_string()))?;
    info!(
        "transcribe ok: {} chars, words_used={}, words_remaining={}",
        text.len(),
        parsed.words_used.unwrap_or(0),
        parsed.words_remaining.unwrap_or(0)
    );

    Ok(TranscriptionResult {
        transcript: text.trim().to_string(),
        latency_ms,
        words_used: parsed.words_used,
        words_remaining: parsed.words_remaining,
    })
}

// MARK: - Gemini (Google AI Studio / Vertex AI)

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsage {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
}

/// Both providers speak the same `generateContent` API and differ only in the
/// endpoint, so the URL is the one thing that branches.
fn gemini_url(prefs: &Prefs) -> Result<String, TranscriptionError> {
    match prefs.provider {
        Provider::VertexAI => {
            let project = prefs.vertex_project_id.trim();
            if project.is_empty() {
                return Err(TranscriptionError::MissingVertexConfig);
            }
            let region = prefs.vertex_region.as_str();
            let host = if region == "global" {
                "aiplatform.googleapis.com".to_string()
            } else {
                format!("{region}-aiplatform.googleapis.com")
            };
            Ok(format!(
                "https://{host}/v1/projects/{project}/locations/{region}/publishers/google/models/{MODEL}:generateContent"
            ))
        }
        _ => Ok(format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent"
        )),
    }
}

async fn transcribe_gemini(
    audio_b64: String,
    system_prompt: String,
    api_key: &str,
    prefs: &Prefs,
) -> Result<TranscriptionResult, TranscriptionError> {
    let url = gemini_url(prefs)?;
    let (audio_chars, prompt_chars) = (audio_b64.len(), system_prompt.len());
    let body = json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": [{
            "role": "user",
            "parts": [{ "inline_data": { "mime_type": "audio/flac", "data": audio_b64 } }],
        }],
    });

    // The key rides in the query string (as the macOS app does); log the URL
    // before it is attached so it never lands in orate.log.
    debug!(
        "POST {url} via {} (base64_chars={}, prompt_chars={})",
        prefs.provider.display_name(),
        audio_chars, prompt_chars
    );

    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    let response = client
        .post(&url)
        .query(&[("key", api_key)])
        .json(&body)
        .send()
        .await?;
    let latency_ms = start.elapsed().as_millis();

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|e| TranscriptionError::Network(format!("reading body: {e}")))?;
    debug!(
        "response: status={} latency={}ms body_chars={}",
        status,
        latency_ms,
        body_text.len()
    );

    if !status.is_success() {
        error!(
            "{} returned {}: {}",
            prefs.provider.display_name(),
            status,
            truncate(&body_text, 1024)
        );
        return Err(TranscriptionError::Api(
            status.as_u16(),
            gemini_error_message(&body_text),
        ));
    }

    let parsed: GeminiResponse = serde_json::from_str(&body_text).map_err(|e| {
        error!(
            "Gemini response did not parse: {e}\n--- body ---\n{}\n--- end ---",
            truncate(&body_text, 1024)
        );
        TranscriptionError::Parse(e.to_string())
    })?;

    // A candidate with no parts is how Gemini answers pure silence, which the
    // prompt asks for, so treat it as an empty transcript rather than an error.
    let candidate = parsed
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| TranscriptionError::Parse(format!("no candidates: {}", truncate(&body_text, 200))))?;
    let text: String = candidate
        .content
        .map(|c| c.parts.into_iter().filter_map(|p| p.text).collect())
        .unwrap_or_default();

    if let Some(usage) = &parsed.usage_metadata {
        info!(
            "transcribe ok: {} chars, prompt_tokens={}, output_tokens={}",
            text.len(),
            usage.prompt_token_count,
            usage.candidates_token_count
        );
    }

    Ok(TranscriptionResult {
        transcript: text.trim().to_string(),
        latency_ms,
        words_used: None,
        words_remaining: None,
    })
}

/// Google errors look like `{"error": {"message": "..."}}`; surface just the
/// message so the overlay isn't a wall of JSON.
fn gemini_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| truncate(body, 200))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push_str("\u{2026}");
        out
    }
}

fn build_system_prompt(custom: Option<&str>, vocab: &[String]) -> String {
    let mut prompt = String::from(SYSTEM_PROMPT);

    if !vocab.is_empty() {
        prompt.push_str(
            "\n\nVocabulary \u{2014} the user has registered these custom words. \
             When you hear something that sounds like one of these words, use the exact spelling provided here:\n",
        );
        for word in vocab {
            prompt.push_str("- ");
            prompt.push_str(word);
            prompt.push('\n');
        }
        prompt.push_str(
            "IMPORTANT: These vocabulary words are spelling hints ONLY. Do not use them to generate or infer content. \
             If no speech is present in the audio, output an empty string regardless of these words.",
        );
    }

    if let Some(custom) = custom {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            prompt.push_str("\n\nUser's custom instructions:\n");
            prompt.push_str(trimmed);
        }
    }

    prompt
}
