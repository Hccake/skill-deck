use std::fmt;

use environment_protocol::MAX_PAYLOAD_CHUNK_BYTES;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferDeclaration {
    pub owner_request_id: u64,
    pub transfer_id: u64,
    pub total_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferCompletion {
    pub owner_request_id: u64,
    pub transfer_id: u64,
    pub total_bytes: u64,
    pub sha256: String,
}

pub struct CompletedInboundTransfer {
    pub declaration: TransferDeclaration,
    pub file: tokio::fs::File,
}

pub struct InboundTransfer {
    declaration: TransferDeclaration,
    received_bytes: u64,
    hasher: Sha256,
    file: tokio::fs::File,
}

#[derive(Debug)]
pub enum InboundTransferError {
    InvalidDeclaration,
    InvalidChunk,
    InvalidCompletion,
    Io(std::io::Error),
}

impl fmt::Display for InboundTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDeclaration => formatter.write_str("invalid inbound transfer declaration"),
            Self::InvalidChunk => formatter.write_str("invalid inbound transfer chunk"),
            Self::InvalidCompletion => formatter.write_str("invalid inbound transfer completion"),
            Self::Io(error) => write!(formatter, "inbound transfer I/O failed: {error}"),
        }
    }
}

impl std::error::Error for InboundTransferError {}

impl From<std::io::Error> for InboundTransferError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl InboundTransfer {
    pub fn begin(
        declaration: TransferDeclaration,
        transfer_limit: u64,
        file: tokio::fs::File,
    ) -> Result<Self, InboundTransferError> {
        if declaration.owner_request_id == 0
            || declaration.transfer_id == 0
            || declaration.total_bytes > transfer_limit
            || !valid_sha256(&declaration.sha256)
        {
            return Err(InboundTransferError::InvalidDeclaration);
        }
        Ok(Self {
            declaration,
            received_bytes: 0,
            hasher: Sha256::new(),
            file,
        })
    }

    pub async fn write_chunk(
        &mut self,
        transfer_id: u64,
        bytes: &[u8],
    ) -> Result<(), InboundTransferError> {
        if transfer_id != self.declaration.transfer_id
            || bytes.is_empty()
            || bytes.len() > MAX_PAYLOAD_CHUNK_BYTES
            || self.received_bytes.saturating_add(bytes.len() as u64) > self.declaration.total_bytes
        {
            return Err(InboundTransferError::InvalidChunk);
        }
        self.file.write_all(bytes).await?;
        self.hasher.update(bytes);
        self.received_bytes += bytes.len() as u64;
        Ok(())
    }

    pub async fn complete(
        mut self,
        completion: TransferCompletion,
    ) -> Result<CompletedInboundTransfer, InboundTransferError> {
        let actual_sha256 = format!("sha256:{:x}", self.hasher.finalize());
        if completion.owner_request_id != self.declaration.owner_request_id
            || completion.transfer_id != self.declaration.transfer_id
            || completion.total_bytes != self.declaration.total_bytes
            || completion.sha256 != self.declaration.sha256
            || self.received_bytes != self.declaration.total_bytes
            || actual_sha256 != self.declaration.sha256
        {
            return Err(InboundTransferError::InvalidCompletion);
        }
        self.file.flush().await?;
        Ok(CompletedInboundTransfer {
            declaration: self.declaration,
            file: self.file,
        })
    }
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
