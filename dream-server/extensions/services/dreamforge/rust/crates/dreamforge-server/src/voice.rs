//! Voice/TTS proxy routes.
//!
//! Proxies audio to Whisper (STT) and Kokoro TTS services running
//! alongside DreamForge in the DreamServer stack.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use serde_json::json;

use crate::AppState;

/// Build voice API routes.
pub fn voice_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/voice/transcribe", post(transcribe))
        .route("/api/voice/speak", post(speak))
        .with_state(state)
}

/// POST /api/voice/transcribe — proxy audio to Whisper STT.
///
/// Expects audio body (wav/webm/mp3). Forwards to Whisper service
/// and returns the transcription text.
async fn transcribe(
    State(_state): State<Arc<AppState>>,
    body: Body,
) -> impl IntoResponse {
    let whisper_url = std::env::var("WHISPER_URL").unwrap_or_else(|_| "http://localhost:8000".into());
    let url = format!("{whisper_url}/asr?output=json");

    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"error": "failed to read audio body"})),
            )
                .into_response();
        }
    };

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new().part(
        "audio_file",
        reqwest::multipart::Part::bytes(body_bytes.to_vec())
            .file_name("audio.webm")
            .mime_str("audio/webm")
            .unwrap_or_else(|_| {
                reqwest::multipart::Part::bytes(body_bytes.to_vec()).file_name("audio.webm")
            }),
    );

    match client.post(&url).multipart(form).send().await {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await.unwrap_or_default();
            axum::Json(json!({"text": text})).into_response()
        }
        Ok(resp) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({"error": format!("whisper returned {}", resp.status())})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({"error": format!("whisper unavailable: {e}")})),
        )
            .into_response(),
    }
}

/// POST /api/voice/speak — proxy text to Kokoro TTS.
///
/// Expects JSON `{"text": "..."}`. Returns audio bytes from TTS service.
async fn speak(
    State(_state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    let tts_url = std::env::var("TTS_URL").unwrap_or_else(|_| "http://localhost:8880".into());
    let text = body["text"].as_str().unwrap_or_default();

    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "text is required"})),
        )
            .into_response();
    }

    let client = reqwest::Client::new();
    match client
        .post(format!("{tts_url}/v1/audio/speech"))
        .json(&json!({
            "model": "kokoro",
            "input": text,
            "voice": "af_heart",
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let audio = resp.bytes().await.unwrap_or_default();
            (
                StatusCode::OK,
                [("content-type", "audio/wav")],
                audio.to_vec(),
            )
                .into_response()
        }
        Ok(resp) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({"error": format!("TTS returned {}", resp.status())})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({"error": format!("TTS unavailable: {e}")})),
        )
            .into_response(),
    }
}
