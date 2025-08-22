// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use bonsai_sdk::responses::{
    CreateSessRes, ImgUploadRes, ProofReq, SessionStats, SessionStatusRes, SnarkReq,
    SnarkStatusRes, UploadRes,
};
use risc0_zkvm::Receipt;
use serde_json::json;
use std::time::Duration;
use tracing::info;

use crate::{
    error::Error,
    prover::{ProverHandle, Task},
    state::{AppState, SessionStatus},
    url_resolver::SharedUrlResolver,
};

pub(crate) async fn get_image_upload(
    State(s): State<AppState>,
    Path(image_id): Path<String>,
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<ImgUploadRes>, Error> {
    let state = &s.read()?;
    match state.get_image(&image_id) {
        Some(_) => Err(Error::ImageIdExists),
        None => {
            let base_url = url_resolver
                .resolve(&headers)
                .map_err(|_| Error::ServerUrlResolution)?;
            Ok(Json(ImgUploadRes {
                url: format!(
                    "{}/images/{}",
                    base_url.as_str().trim_end_matches('/'),
                    image_id
                ),
            }))
        }
    }
}

pub(crate) async fn put_image_upload(
    State(s): State<AppState>,
    Path(image_id): Path<String>,
    body: Bytes,
) -> Result<(), Error> {
    s.write()?.put_image(image_id.clone(), body.to_vec());
    info!("ImageID {image_id} uploaded");
    Ok(())
}

pub(crate) async fn get_input_upload(
    State(s): State<AppState>,
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<UploadRes>, Error> {
    let _state = &s.read()?;
    let input_id = uuid::Uuid::new_v4();
    let base_url = url_resolver
        .resolve(&headers)
        .map_err(|_| Error::ServerUrlResolution)?;
    Ok(Json(UploadRes {
        url: format!(
            "{}/inputs/{}",
            base_url.as_str().trim_end_matches('/'),
            input_id
        ),
        uuid: input_id.to_string(),
    }))
}

pub(crate) async fn put_input_upload(
    State(s): State<AppState>,
    Path(input_id): Path<String>,
    body: Bytes,
) -> Result<(), Error> {
    s.write()?.put_input(input_id, body.to_vec());
    Ok(())
}

pub(crate) async fn create_session(
    Extension(prover_handle): Extension<ProverHandle>,
    State(s): State<AppState>,
    Json(request): Json<ProofReq>,
) -> Result<Json<CreateSessRes>, Error> {
    let session_id = uuid::Uuid::new_v4();
    info!("create_session: {}", session_id);
    s.write()?
        .put_session(session_id.to_string(), SessionStatus::Running, None);
    let task = Task {
        image_id: request.img,
        input_id: request.input,
        session_id: session_id.to_string(),
        assumptions: request.assumptions,
    };
    prover_handle
        .execute(task, Duration::from_secs(120))
        .await?;

    Ok(Json(CreateSessRes {
        uuid: session_id.to_string(),
    }))
}

pub(crate) async fn session_status(
    State(s): State<AppState>,
    Path(session_id): Path<String>,
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<SessionStatusRes>, Error> {
    let storage = s.read()?;
    let (status, stats) = storage
        .get_session(&session_id)
        .ok_or_else(|| anyhow::anyhow!("Session not found for session id: {:?}", &session_id))?;
    let receipt = storage.get_receipt(&session_id);
    let stats = stats.as_ref().map(|stats| SessionStats {
        segments: stats.segments,
        total_cycles: stats.total_cycles,
        cycles: stats.user_cycles,
    });
    match receipt {
        Some(_) => {
            let base_url = url_resolver
                .resolve(&headers)
                .map_err(|_| Error::ServerUrlResolution)?;
            Ok(Json(SessionStatusRes {
                status: status.to_string(),
                receipt_url: Some(format!(
                    "{}/receipts/{}",
                    base_url.as_str().trim_end_matches('/'),
                    session_id
                )),
                error_msg: None,
                state: None,
                elapsed_time: None,
                stats,
            }))
        }
        None => Ok(Json(SessionStatusRes {
            status: status.to_string(),
            receipt_url: None,
            error_msg: None,
            state: None,
            elapsed_time: None,
            stats: None,
        })),
    }
}

pub(crate) async fn create_snark(
    Json(request): Json<SnarkReq>,
) -> Result<Json<CreateSessRes>, Error> {
    info!("create_snark: {}", request.session_id);
    Ok(Json(CreateSessRes {
        uuid: request.session_id,
    }))
}

pub(crate) async fn snark_status(
    State(s): State<AppState>,
    Path(snark_id): Path<String>,
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<SnarkStatusRes>, Error> {
    let storage = s.read()?;
    storage
        .get_session(&snark_id)
        .ok_or_else(|| anyhow::anyhow!("Snark status not found for snark id: {:?}", &snark_id))?;
    let receipt = storage.get_receipt(&snark_id);
    match receipt {
        Some(bytes) => {
            let _receipt: Receipt = bincode::deserialize(&bytes)?;
            let base_url = url_resolver
                .resolve(&headers)
                .map_err(|_| Error::ServerUrlResolution)?;
            Ok(Json(SnarkStatusRes {
                status: SessionStatus::Succeeded.to_string(),
                output: Some(format!(
                    "{}/receipts/{}",
                    base_url.as_str().trim_end_matches('/'),
                    snark_id
                )),
                error_msg: None,
            }))
        }
        None => Ok(Json(SnarkStatusRes {
            status: SessionStatus::Running.to_string(),
            output: None,
            error_msg: None,
        })),
    }
}

pub(crate) async fn get_receipt(
    State(s): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Vec<u8>, Error> {
    info!("get_receipt: {}", session_id);
    let storage = s.read()?;
    let receipt = storage
        .get_receipt(&session_id)
        .ok_or_else(|| anyhow::anyhow!("Receipt not found for session id: {:?}", &session_id))?;
    Ok(receipt)
}

pub(crate) async fn get_receipt_upload(
    State(s): State<AppState>,
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<UploadRes>, Error> {
    let _state = &s.read()?;
    let receipt_id = uuid::Uuid::new_v4();
    let base_url = url_resolver
        .resolve(&headers)
        .map_err(|_| Error::ServerUrlResolution)?;
    Ok(Json(UploadRes {
        url: format!(
            "{}/receipts/{}",
            base_url.as_str().trim_end_matches('/'),
            receipt_id
        ),
        uuid: receipt_id.to_string(),
    }))
}

pub(crate) async fn put_receipt(
    State(s): State<AppState>,
    Path(receipt_id): Path<String>,
    body: Bytes,
) -> Result<(), Error> {
    s.write()?.put_receipt(receipt_id.clone(), body.to_vec());
    Ok(())
}

pub(crate) async fn health_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "message": "Bonsai REST API is running"
        })),
    )
}

pub(crate) async fn resolved_server_url(
    Extension(url_resolver): Extension<SharedUrlResolver>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Error> {
    let resolved_url = url_resolver
        .resolve(&headers)
        .map_err(|_| Error::ServerUrlResolution)?;

    Ok(Json(json!({
        "resolved_server_url": resolved_url.to_string(),
        "headers": {
            "forwarded": headers.get("forwarded").and_then(|v| v.to_str().ok()),
            "x-forwarded-proto": headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()),
            "x-forwarded-host": headers.get("x-forwarded-host").and_then(|v| v.to_str().ok()),
            "x-forwarded-port": headers.get("x-forwarded-port").and_then(|v| v.to_str().ok()),
        }
    })))
}
