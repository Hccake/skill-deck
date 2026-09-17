use sha2::{Digest, Sha256};
use wsl_environment_worker::inbound_transfer::{
    InboundTransfer, TransferCompletion, TransferDeclaration,
};

#[tokio::test]
async fn transfer_streams_to_disk_and_verifies_its_completion() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let file = tokio::fs::File::from_std(temp.reopen().unwrap());
    let payload = b"payload-bytes";
    let sha256 = format!("sha256:{:x}", Sha256::digest(payload));
    let mut transfer = InboundTransfer::begin(
        TransferDeclaration {
            owner_request_id: 7,
            transfer_id: 11,
            total_bytes: payload.len() as u64,
            sha256: sha256.clone(),
        },
        1024,
        file,
    )
    .unwrap();

    transfer.write_chunk(11, &payload[..4]).await.unwrap();
    transfer.write_chunk(11, &payload[4..]).await.unwrap();
    let completed = transfer
        .complete(TransferCompletion {
            owner_request_id: 7,
            transfer_id: 11,
            total_bytes: payload.len() as u64,
            sha256,
        })
        .await
        .unwrap();
    drop(completed.file);

    assert_eq!(std::fs::read(temp.path()).unwrap(), payload);
}
