use bitcoin::BlockHeader;
use common_bitcoin::adapter::Adapter;
use common_bitcoin::error::ContractResult;
use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::{Deserialize, Serialize};
use cosmwasm_std::from_json;

use crate::constants::{
    MAX_LENGTH, MAX_TARGET, MAX_TIME_INCREASE, RETARGET_INTERVAL, TARGET_SPACING, TARGET_TIMESPAN,
};
use crate::header::{WorkHeader, WrappedHeader};

///  HeaderConfiguration parameters for Bitcoin header processing.
// TODO: implement trait that returns constants for bitcoin::Network variants
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "cosmwasm_schema::serde")]
#[schemars(crate = "cosmwasm_schema::schemars")]
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

impl HeaderConfig {
    pub fn mainnet() -> ContractResult<Self> {
        Self::from_bytes(include_bytes!("checkpoint.json"), false)
    }

    pub fn testnet() -> ContractResult<Self> {
        Self::from_bytes(include_bytes!("testnet-checkpoint.json"), true)
    }

    pub fn from_bytes(checkpoint_json: &[u8], min_difficulty_blocks: bool) -> ContractResult<Self> {
        let checkpoint: (u32, BlockHeader) = from_json(checkpoint_json)?;
        let (height, header) = checkpoint;

        Ok(Self {
            max_length: MAX_LENGTH,
            max_time_increase: MAX_TIME_INCREASE,
            trusted_height: height,
            retarget_interval: RETARGET_INTERVAL,
            target_spacing: TARGET_SPACING,
            target_timespan: TARGET_TIMESPAN,
            max_target: MAX_TARGET,
            trusted_header: header.into(),
            retargeting: true,
            min_difficulty_blocks,
        })
    }

    pub fn work_header(&self) -> WorkHeader {
        let decoded_adapter: Adapter<BlockHeader> = self.trusted_header.into();
        let wrapped_header = WrappedHeader::new(decoded_adapter, self.trusted_height);
        let work_header = WorkHeader::new(wrapped_header.clone(), wrapped_header.work());
        work_header
    }
}
