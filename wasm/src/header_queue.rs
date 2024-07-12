use crate::{
    adapter::Adapter, error::ContractResult, MAX_LENGTH, MAX_TARGET, MAX_TIME_INCREASE,
    RETARGET_INTERVAL, TARGET_SPACING, TARGET_TIMESPAN,
};
use bitcoin::{util::uint::Uint256, BlockHash, BlockHeader, TxMerkleNode};
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

///  HeaderConfiguration parameters for Bitcoin header processing.
// TODO: implement trait that returns constants for bitcoin::Network variants
#[derive(Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct HeaderConfig {
    /// The maximum number of headers that can be stored in the header queue
    /// before pruning.
    pub max_length: u64,
    /// The maximum amount of time (in seconds) that can pass between the
    /// timestamp of the last header in the header queue and the timestamp of
    /// the next header to be added.
    pub max_time_increase: u32,
    /// The height of the trusted header.
    pub trusted_height: u32,
    /// The interval (in blocks) at which the difficulty target is adjusted.
    pub retarget_interval: u32,
    /// The target time interval (in seconds) between blocks.
    pub target_spacing: u32,
    /// The target amount of time (in seconds) that should pass between the
    /// timestamps of the first and last header in a retargeting period. This
    /// should be equivalent to `retarget_interval * target_spacing`.
    // TODO: derive from `retarget_interval` and `target_spacing`
    pub target_timespan: u32,
    /// The maximum target value.
    pub max_target: u32,
    /// Whether or not the header queue should retarget difficulty.
    pub retargeting: bool,
    /// Whether or not the header queue should drop back down to the minimum
    /// difficulty after a certain amount of time has passed (used in Bitcoin
    /// testnet).
    pub min_difficulty_blocks: bool,
    /// The trusted header (the header which populates the queue when it is
    /// newly created), as encoded bytes.    
    pub trusted_header: Adapter<BlockHeader>,
}

/// A `WrappedHeader`, along with a total estimated amount of work (measured in
/// hashes) done in the header and previous headers.
#[derive(Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WorkHeader {
    pub chain_work: Adapter<Uint256>,
    pub header: WrappedHeader,
}

impl WorkHeader {
    /// Create a new `WorkHeader`` from a `WrappedHeader` and a `Uint256`.
    pub fn new(header: WrappedHeader, chain_work: Uint256) -> WorkHeader {
        WorkHeader {
            header,
            chain_work: Adapter::new(chain_work),
        }
    }

    /// The timestamp of the block header.
    pub fn time(&self) -> u32 {
        self.header.time()
    }

    /// The target - the value the hash must be less than to be valid
    /// proof-of-work.
    pub fn block_hash(&self) -> BlockHash {
        self.header.block_hash()
    }

    /// The estimated amount of work (measured in hashes) done in the header,
    /// not including work done in any previous headers.
    pub fn work(&self) -> Uint256 {
        self.header.work()
    }

    /// The height of the block header.
    pub fn height(&self) -> u32 {
        self.header.height()
    }

    /// The Merkle root of the block header.
    pub fn merkle_root(&self) -> TxMerkleNode {
        self.header.header.merkle_root
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct HeaderQueue {
    pub(crate) current_work: Adapter<Uint256>,
    pub(crate) config: HeaderConfig,
}

/// A wrapper around a bitcoin::BlockHeader that implements the core orga
/// traits, and includes the block's height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WrappedHeader {
    height: u32,
    header: Adapter<BlockHeader>,
}

impl WrappedHeader {
    /// Create a new WrappedHeader from an Adapter<bitcoin::BlockHeader> and a
    /// height.
    pub fn new(header: Adapter<BlockHeader>, height: u32) -> Self {
        WrappedHeader { height, header }
    }

    /// Create a new WrappedHeader from a bitcoin::BlockHeader and a height.    
    pub fn from_header(header: &BlockHeader, height: u32) -> Self {
        WrappedHeader {
            height,
            header: Adapter::new(*header),
        }
    }

    /// The timestamp of the block header.
    pub fn time(&self) -> u32 {
        self.header.time
    }

    /// The target - the value the hash must be less than to be valid
    /// proof-of-work.
    pub fn target(&self) -> Uint256 {
        self.header.target()
    }

    /// The block hash.
    pub fn block_hash(&self) -> BlockHash {
        self.header.block_hash()
    }

    /// The previous block hash.
    pub fn prev_blockhash(&self) -> BlockHash {
        self.header.prev_blockhash
    }

    /// The total estimated number of work (measured in hashes) represented by
    /// the block header's proof-of-work.
    pub fn work(&self) -> Uint256 {
        self.header.work()
    }

    /// The height of the block header.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The target, in compact form.
    pub fn bits(&self) -> u32 {
        self.header.bits
    }

    /// Converts a compact target to a Uint256.
    pub fn u256_from_compact(compact: u32) -> Uint256 {
        BlockHeader::u256_from_compact_target(compact)
    }

    /// Converts a Uint256 to a compact target.
    pub fn compact_target_from_u256(target: &Uint256) -> u32 {
        BlockHeader::compact_target_from_u256(target)
    }

    /// Converts a u32 to a Uint256.
    pub fn u32_to_u256(value: u32) -> Uint256 {
        let bytes = value.to_be_bytes();
        let mut buffer = [0u8; 32];
        buffer[32 - bytes.len()..].copy_from_slice(&bytes);

        Uint256::from_be_bytes(buffer)
    }

    /// Validates the proof-of-work of the block header, returning an error if
    /// the proof-of-work is invalid.
    pub fn validate_pow(&self, required_target: &Uint256) -> ContractResult<BlockHash> {
        Ok(self.header.validate_pow(required_target)?)
    }
}

#[wasm_bindgen]
pub fn newWrappedHeader(header: BlockHeader, height: u32) -> WrappedHeader {
    WrappedHeader::new(header.into(), height)
}

#[wasm_bindgen]
pub fn newHeaderConfig(height: u32, block_header: JsValue) -> ContractResult<HeaderConfig> {
    // because BlockHeader is not tsify
    let header: BlockHeader = serde_wasm_bindgen::from_value(block_header)?;

    Ok(HeaderConfig {
        max_length: MAX_LENGTH,
        max_time_increase: MAX_TIME_INCREASE,
        trusted_height: height,
        retarget_interval: RETARGET_INTERVAL,
        target_spacing: TARGET_SPACING,
        target_timespan: TARGET_TIMESPAN,
        max_target: MAX_TARGET,
        trusted_header: header.into(),
        retargeting: true,
        min_difficulty_blocks: false,
    })
}

#[wasm_bindgen]
pub fn newWorkHeader(header_config: HeaderConfig) -> WorkHeader {
    let decoded_adapter: Adapter<BlockHeader> = header_config.trusted_header.into();
    let wrapped_header = WrappedHeader::new(decoded_adapter, header_config.trusted_height);
    let work_header = WorkHeader::new(wrapped_header.clone(), wrapped_header.work());
    work_header
}
