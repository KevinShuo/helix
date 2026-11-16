//! Bounded, authenticated control protocol. Source code stays on the HTTP path.
use anyhow::{ensure, Context, Result};
use helix_view::ai::AiProviderConfig;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) const VERSION: u32 = 1;
const MAX_FRAME: usize = 65_536;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Discovery {
    pub version: u32,
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Envelope {
    pub version: u32,
    pub token: String,
    pub request: Request,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(super) enum Request {
    Register {
        resident: Option<AiProviderConfig>,
    },
    Acquire {
        model: AiProviderConfig,
        restore: Option<AiProviderConfig>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(super) enum Response {
    Ok,
    Error { message: String },
}

impl Response {
    pub fn result(self) -> Result<()> {
        match self {
            Self::Ok => Ok(()),
            Self::Error { message } => anyhow::bail!("{message}"),
        }
    }
}

pub(super) async fn read<T: DeserializeOwned>(
    stream: &mut (impl AsyncReadExt + Unpin),
) -> Result<T> {
    let size = stream.read_u32().await? as usize;
    ensure!(size <= MAX_FRAME, "AI manager message exceeds size limit");
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).context("Invalid AI manager message")
}

pub(super) async fn write<T: Serialize>(
    stream: &mut (impl AsyncWriteExt + Unpin),
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= MAX_FRAME,
        "AI manager message exceeds size limit"
    );
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

pub(super) fn random_token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Could not initialize AI manager token"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
