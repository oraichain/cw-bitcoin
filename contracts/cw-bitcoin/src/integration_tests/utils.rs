use std::path::PathBuf;

use crate::{
    adapter::Adapter, error::ContractResult, header::WrappedHeader, threshold_sig::Signature,
};
use bitcoin::{BlockHash, BlockHeader};
use bitcoincore_rpc_async::{Auth, Client as BitcoinRpcClient, RpcApi};

pub fn retry<F, T, E>(f: F, max_retries: u32) -> std::result::Result<T, E>
where
    F: Fn() -> std::result::Result<T, E>,
{
    let mut retries = 0;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => {
                if retries >= max_retries {
                    return Err(e);
                }
                retries += 1;
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}

pub async fn test_bitcoin_client(rpc_url: String, cookie_file: PathBuf) -> BitcoinRpcClient {
    BitcoinRpcClient::new(rpc_url, Auth::CookieFile(cookie_file))
        .await
        .unwrap()
}

pub async fn get_wrapped_header_from_block_hash(
    btc_client: &BitcoinRpcClient,
    block_hash: &BlockHash,
) -> WrappedHeader {
    let header_info = btc_client.get_block_header_info(block_hash).await.unwrap();
    let height = header_info.height as u32;
    let header = btc_client
        .get_block_header(&header_info.hash)
        .await
        .unwrap();
    WrappedHeader::new(Adapter::new(header), height)
}

#[derive(Debug)]
pub struct BitcoinBlockData {
    pub height: u32,
    pub block_header: BlockHeader,
}

pub async fn populate_bitcoin_block(client: &BitcoinRpcClient) -> BitcoinBlockData {
    let tip_hash = client.get_best_block_hash().await.unwrap();
    let tip_header = client.get_block_header(&tip_hash).await.unwrap();

    let tip_height = client
        .get_block_header_info(&tip_hash)
        .await
        .unwrap()
        .height;

    BitcoinBlockData {
        height: tip_height as u32,
        block_header: tip_header,
    }
}
