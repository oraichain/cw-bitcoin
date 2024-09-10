use bitcoin::blockdata::block::BlockHeader;
use bitcoin::util::uint::Uint256;
use bitcoin::{BlockHash, TxMerkleNode};
use common_bitcoin::adapter::Adapter;
use common_bitcoin::error::ContractResult;
use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::{Deserialize, Serialize};

/// A wrapper around a bitcoin::BlockHeader that implements the core orga
/// traits, and includes the block's height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "cosmwasm_schema::serde")]
#[schemars(crate = "cosmwasm_schema::schemars")]
pub struct WrappedHeader {
    pub height: u32,
    pub header: Adapter<BlockHeader>,
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

/// A `WrappedHeader`, along with a total estimated amount of work (measured in
/// hashes) done in the header and previous headers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "cosmwasm_schema::serde")]
#[schemars(crate = "cosmwasm_schema::schemars")]
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
