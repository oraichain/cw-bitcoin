use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub enum BondStatus {
    /// UNSPECIFIED defines an invalid validator status.
    Unspecified = 0,
    /// UNBONDED defines a validator that is not bonded.
    Unbonded = 1,
    /// UNBONDING defines a validator that is unbonding.
    Unbonding = 2,
    /// BONDED defines a validator that is bonded.
    Bonded = 3,
}

#[cw_serde]
pub struct ValidatorInfo {
    pub operator_address: String,
    pub consensus_pubkey: Vec<u8>,
    pub jailed: bool,
    pub status: i32,
    pub tokens: Uint128,
}
