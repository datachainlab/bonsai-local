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

mod error;
mod prover;
mod routes;
mod state;
pub mod version;

use crate::{
    prover::{Prover, ProverHandle},
    routes::{
        create_session, create_snark, get_image_upload, get_input_upload, get_receipt,
        get_receipt_upload, health_check, put_image_upload, put_input_upload, put_receipt,
        session_status, snark_status,
    },
    state::BonsaiState,
};
use anyhow::Context;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post, put},
    Extension, Router,
};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::{net::TcpListener, sync::mpsc, time};
use tower_http::trace::{DefaultOnRequest, TraceLayer};
use tracing::{info, Level};
use url::Url;

pub struct ServerOptions {
    pub url: Url,
    pub ttl: Duration,
    pub channel_buffer_size: usize,
}

fn app(state: Arc<RwLock<BonsaiState>>, prover_handle: ProverHandle) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/images/upload/:image_id", get(get_image_upload))
        .route("/images/:image_id", put(put_image_upload))
        .route("/inputs/upload", get(get_input_upload))
        .route("/inputs/:input_id", put(put_input_upload))
        .route("/sessions/create", post(create_session))
        .route("/sessions/status/:session_id", get(session_status))
        .route("/snark/create", post(create_snark))
        .route("/snark/status/:snark_id", get(snark_status))
        .route("/receipts/:session_id", get(get_receipt))
        .route("/receipts/:session_id", put(put_receipt))
        .route("/receipts/upload", get(get_receipt_upload))
        .layer(Extension(prover_handle))
        .with_state(state)
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .layer(TraceLayer::new_for_http().on_request(
            DefaultOnRequest::new().level(Level::TRACE), // make on_request less visible
        ))
}

pub async fn serve(listener: TcpListener, options: ServerOptions) -> anyhow::Result<()> {
    let local_addr = listener.local_addr().unwrap();
    let state = Arc::new(RwLock::new(BonsaiState::new(options.url, options.ttl)));

    let (sender, receiver) = mpsc::channel(options.channel_buffer_size);
    let mut prover = Prover::new(receiver, Arc::clone(&state));

    let prover_handle = ProverHandle { sender };

    tokio::spawn(async move { prover.run().await });

    // Start cleanup task
    let cleanup_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60)); // Run cleanup every minute
        loop {
            interval.tick().await;
            if let Ok(mut state) = cleanup_state.write() {
                state.cleanup_expired();
                info!("Cleaned up expired entries");
            }
        }
    });

    info!("Bonsai started on {local_addr}");

    axum::serve(listener, app(state, prover_handle))
        .await
        .context(format!("failed to serve Bonsai API on {local_addr}"))
}

#[cfg(test)]
mod test {
    use crate::{serve, state::SessionStatus, ServerOptions};
    use anyhow::{bail, Result};
    use risc0_zkvm::compute_image_id;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use url::Url;

    async fn run_bonsai(bonsai_api_url: String, bonsai_api_key: String, elf: &[u8]) -> Result<()> {
        let client = bonsai_sdk::non_blocking::Client::from_parts(
            bonsai_api_url,
            bonsai_api_key,
            risc0_zkvm::VERSION,
        )?;

        // Compute the image_id, then upload the ELF with the image_id as its key.
        // TODO: it would be nice if `bonsai_sdk::upload_img` only took the ELF
        // so that the image_id can be computed server-side.
        let image_id = hex::encode(compute_image_id(elf)?);
        client.upload_img(&image_id, elf.to_vec()).await?;

        // Prepare input data and upload it.
        let input_id = client.upload_input(vec![]).await?;

        // Prepare symbolic list of receipt data and upload it.
        let receipts_ids = vec![client.upload_receipt(vec![]).await?];

        // Start a session running the prover
        let session = client
            .create_session(image_id, input_id, receipts_ids, false)
            .await?;
        loop {
            let res = session.status(&client).await?;
            if res.status == SessionStatus::Running.to_string() {
                std::thread::sleep(Duration::from_secs(15));
                continue;
            }
            if res.status == SessionStatus::Succeeded.to_string() {
                // Download the receipt, containing the output
                let receipt_url = res
                    .receipt_url
                    .expect("API error, missing receipt on completed session");
                client.download(&receipt_url).await.unwrap();
            } else {
                bail!("Error");
            }

            break;
        }

        Ok(())
    }

    // #[tokio::test]
    // async fn local_bonsai() {
    //     use std::{thread::sleep, time::Duration};

    //     let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    //     let local_addr = listener.local_addr().unwrap();
    //     let url = Url::parse(&format!("http://{}", local_addr)).unwrap();
    //     let ttl = Duration::from_secs(3600); // 1 hour for tests
    //     let local_bonsai_handle = tokio::spawn(async move { serve(listener, url, ttl).await });

    //     // wait for the service to be up
    //     sleep(Duration::from_secs(1));

    //     run_bonsai(
    //         format!("http://{local_addr}"),
    //         "test_key".to_string(),
    //         HELLO_COMMIT_ELF,
    //     )
    //     .await
    //     .unwrap();

    //     local_bonsai_handle.abort();
    // }

    #[tokio::test]
    async fn local_bonsai_wrong_elf() {
        use std::{thread::sleep, time::Duration};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        let url = Url::parse(&format!("http://{}", local_addr)).unwrap();
        let options = ServerOptions {
            url,
            ttl: Duration::from_secs(3600), // 1 hour for tests
            channel_buffer_size: 8,
        };
        let local_bonsai_handle = tokio::spawn(async move { serve(listener, options).await });

        // wait for the service to be up
        sleep(Duration::from_secs(1));

        assert!(run_bonsai(
            format!("http://{local_addr}"),
            "test_key".to_string(),
            b"wrong ELF"
        )
        .await
        .is_err());

        local_bonsai_handle.abort();
    }
}
