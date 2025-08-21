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

use risc0_zkvm::Prover as _;
use risc0_zkvm::{ExecutorEnv, LocalProver, ProveInfo, ProverOpts, Receipt, VerifierContext};
use std::{
    fmt,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::state::SessionStatus;
use crate::{error::Error, state::BonsaiState};

#[derive(Debug, Clone)]
pub(crate) struct Task {
    pub session_id: String,
    pub image_id: String,
    pub input_id: String,
    pub assumptions: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum ProverMessage {
    RunSession(Task),
}

impl fmt::Display for ProverMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProverMessage::RunSession(task) => {
                write!(f, "ProverMessage::RunSession: {{ task: {task:?} }}")
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ProverHandle {
    pub sender: mpsc::Sender<ProverMessage>,
}

impl ProverHandle {
    pub(crate) async fn execute(
        &self,
        task: Task,
        timeout_duration: Duration,
    ) -> Result<(), Error> {
        let msg = ProverMessage::RunSession(task);

        match tokio::time::timeout(timeout_duration, self.sender.send(msg)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                error!(
                    "receiver dropped: is_closed={} error={:?}",
                    self.sender.is_closed(),
                    e
                );
                Err(Error::Unspecified(anyhow::anyhow!(
                    "Prover receiver dropped"
                )))
            }
            Err(e) => {
                warn!(
                    "Prover queue is full, timeout after {:?}, error={:?}",
                    timeout_duration, e
                );
                Err(Error::ProverQueueFull)
            }
        }
    }
}

pub(crate) struct Prover {
    pub(crate) receiver: mpsc::Receiver<ProverMessage>,
    pub(crate) storage: Arc<RwLock<BonsaiState>>,
}

impl Prover {
    pub(crate) fn new(
        receiver: mpsc::Receiver<ProverMessage>,
        storage: Arc<RwLock<BonsaiState>>,
    ) -> Self {
        Prover { receiver, storage }
    }

    pub async fn handle_message(&mut self, msg: &ProverMessage) -> Result<(), Error> {
        match msg {
            ProverMessage::RunSession(task) => {
                info!("Running task...");
                let image = self.get_image(task).await?;
                let input = self.get_input(task).await?;
                let receipts = self.get_receipts(task).await?;
                let elf = image.as_slice();

                let mut env = ExecutorEnv::builder();
                for receipt in receipts {
                    if receipt.is_empty() {
                        continue;
                    }
                    let deserialized_receipt: Receipt = bincode::deserialize(&receipt)?;
                    env.add_assumption(deserialized_receipt);
                }

                let env = env
                    .write_slice(&input)
                    .session_limit(None)
                    .segment_limit_po2(20)
                    .build()
                    .map_err(|e| {
                        anyhow::anyhow!("failed to build executor environment: {:?}", e)
                    })?;
                let receipt = self.prove(env, elf)?;
                let receipt_bytes = bincode::serialize(&receipt.receipt)?;
                self.storage
                    .write()?
                    .put_receipt(task.session_id.clone(), receipt_bytes);
                self.storage.write()?.put_session(
                    task.session_id.clone(),
                    SessionStatus::Succeeded,
                    Some(receipt.stats),
                );
            }
        }

        Ok(())
    }

    fn prove(&self, env: ExecutorEnv, elf: &[u8]) -> Result<ProveInfo, Error> {
        let prover = LocalProver::new("bonsai");
        let prover_info = prover.prove_with_ctx(
            env,
            &VerifierContext::default(),
            elf,
            &ProverOpts::groth16(),
        )?;
        Ok(prover_info)
    }

    pub(crate) async fn run(&mut self) -> Result<(), Error> {
        while let Some(msg) = self.receiver.recv().await {
            info!("Received message: {}", &msg);
            match self.handle_message(&msg).await {
                Ok(_) => match &msg {
                    ProverMessage::RunSession(task) => {
                        info!("Task done: {:?}", task.session_id)
                    }
                },
                Err(err) => {
                    match &msg {
                        ProverMessage::RunSession(task) => self.storage.write()?.put_session(
                            task.session_id.clone(),
                            SessionStatus::Failed,
                            None,
                        ),
                    };
                    error!("Task {} failed! - {:?}", msg, err)
                }
            }
        }
        Ok(())
    }

    async fn get_image(&self, task: &Task) -> Result<Vec<u8>, Error> {
        Ok(self
            .storage
            .read()?
            .get_image(&task.image_id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get image for ID: {:?}", task.image_id))?)
    }

    async fn get_input(&self, task: &Task) -> Result<Vec<u8>, Error> {
        Ok(self
            .storage
            .read()?
            .get_input(&task.input_id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get input for ID: {:?}", task.input_id))?)
    }

    async fn get_receipts(&self, task: &Task) -> Result<Vec<Vec<u8>>, Error> {
        let mut assumptions: Vec<Vec<u8>> = vec![];
        for receipt_id in &task.assumptions {
            let receipt = self
                .storage
                .read()?
                .get_receipt(receipt_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("Failed to get input for ID: {:?}", task.input_id)
                })?;
            assumptions.push(receipt);
        }
        Ok(assumptions)
    }
}
