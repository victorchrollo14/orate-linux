use base64::Engine;
use serde::{Deserialize, Serialize};

const ORATE_CLOUD_BASE: &str = "https://orate-api.lavisht22.workers.dev";
pub const MODEL: &str = "gemini-3.1-flash-lite-preview";

const SYSTEM_PROMPT: &str = "You are Orate, an intelligent speech-to-text assistant. Your job is to transcribe spoken audio and produce clean, polished text ready to be inserted directly into whatever the user is typing.\n\nCore rules:\n- Transcribe the spoken content accurately, preserving the speaker's intended meaning.\n- Clean up speech disfluencies: remove filler words (um, uh, like, you know), false starts, and repeated words \u{2014} unless they are clearly intentional for emphasis.\n- Fix grammar and punctuation naturally. Add proper capitalization, periods, commas, and other punctuation as appropriate for written text.\n- Do NOT add any preamble, commentary, labels, or formatting beyond the transcription itself. Output ONLY the final clean text.\n- Do NOT wrap the output in quotes or add \"Transcription:\" or similar prefixes.\n- If the speaker dictates punctuation explicitly (e.g. says \"period\", \"comma\", \"new line\", \"question mark\"), convert those to the actual punctuation characters.\n- CRITICAL: If the audio contains no spoken words (silence, background noise, breathing, typing, or other non-speech sounds), you MUST output an empty string. Do not generate any text whatsoever \u{2014} not even from vocabulary hints or custom instructions. Only transcribe actual spoken words.\n- Preserve the speaker's tone and intent: if they are writing a casual message, keep it casual. If formal, keep it formal.\n- For numbers, use digits for quantities and measurements (e.g. \"5 minutes\", \"200 users\") and words for conversational usage (e.g. \"a couple of things\").\n- If the speaker is dictating a list (e.g. \"first... second... third...\" or \"number one... number two...\" or \"bullet point...\"), format the output as a properly structured list with line breaks and markers (1. 2. 3. or - bullets) as appropriate.";

#[derive(Debug)]
pub struct TranscriptionResult {
    pub transcript: String,
    pub latency_ms: u128,
    pub words_used: u64,
    pub words_remaining: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error("missing API key \u{2014} open Orate to add your Orate Cloud key")]
    MissingApiKey,
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
    custom_instructions: Option<&str>,
    vocabulary: &[String],
) -> Result<TranscriptionResult, TranscriptionError> {
    if api_key.is_empty() {
        return Err(TranscriptionError::MissingApiKey);
    }

    let body = OrateRequest {
        audio: base64::engine::general_purpose::STANDARD.encode(audio),
        system_prompt: build_system_prompt(custom_instructions, vocabulary),
    };

    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    let response = client
        .post(format!("{ORATE_CLOUD_BASE}/transcribe"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let latency_ms = start.elapsed().as_millis();

    let status = response.status();
    let parsed: OrateResponse = response
        .json()
        .await
        .map_err(|e| TranscriptionError::Parse(e.to_string()))?;

    if let Some(err) = parsed.error {
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

    Ok(TranscriptionResult {
        transcript: text.trim().to_string(),
        latency_ms,
        words_used: parsed.words_used.unwrap_or(0),
        words_remaining: parsed.words_remaining.unwrap_or(0),
    })
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
