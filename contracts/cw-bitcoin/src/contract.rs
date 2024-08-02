#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;

use crate::{
    app::Bitcoin,
    entrypoints::*,
    error::ContractError,
    interface::{BitcoinConfig, CheckpointConfig, Config, HeaderConfig},
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg},
    state::{
        BITCOIN_CONFIG, BUILDING_INDEX, CHECKPOINT_CONFIG, CONFIG, FEE_POOL, HEADER_CONFIG,
        VALIDATORS,
    },
};

use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, WasmMsg,
};
use cw2::set_contract_version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cw_bitcoin";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    CONFIG.save(
        deps.storage,
        &Config {
            owner: info.sender,
            token_factory_addr: msg.token_factory_addr,
        },
    )?;

    HEADER_CONFIG.save(deps.storage, &HeaderConfig::mainnet()?)?;
    CHECKPOINT_CONFIG.save(deps.storage, &CheckpointConfig::default())?;
    BITCOIN_CONFIG.save(deps.storage, &&BitcoinConfig::default())?;
    FEE_POOL.save(deps.storage, &0)?;
    BUILDING_INDEX.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RelayDeposit {
            btc_tx,
            btc_height,
            btc_proof,
            btc_vout,
            sigset_index,
            dest,
        } => relay_deposit(
            env,
            deps.storage,
            btc_tx,
            btc_height,
            btc_proof,
            btc_vout,
            sigset_index,
            dest,
        ),
        ExecuteMsg::WithdrawToBitcoin { script_pubkey } => {
            withdraw_to_bitcoin(deps.storage, info, env, script_pubkey)
        }
        ExecuteMsg::RelayHeaders { headers } => relay_headers(deps.storage, headers),
        ExecuteMsg::UpdateHeaderConfig { config } => update_header_config(deps.storage, config),
        ExecuteMsg::UpdateBitcoinConfig { config } => update_bitcoin_config(deps.storage, config),
        ExecuteMsg::UpdateCheckpointConfig { config } => {
            update_checkpoint_config(deps.storage, config)
        }
        ExecuteMsg::SubmitCheckpointSignature {
            xpub,
            sigs,
            checkpoint_index,
            btc_height,
        } => submit_checkpoint_signature(deps.storage, xpub, sigs, checkpoint_index, btc_height),
        ExecuteMsg::SubmitRecoverySignature { xpub, sigs } => {
            submit_recovery_signature(deps.querier, deps.storage, xpub, sigs)
        }
        ExecuteMsg::SetSignatoryKey { xpub } => set_signatory_key(deps.storage, info, xpub),
        ExecuteMsg::AddValidators { addrs, infos } => {
            add_validators(deps.storage, info, addrs, infos)
        }
        ExecuteMsg::RegisterDenom { subdenom, metadata } => {
            register_denom(deps.storage, info, subdenom, metadata)
        }
        ExecuteMsg::SetRecoveryScript { signatory_script } => {
            set_recovery_script(deps.storage, info, signatory_script)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::ClockEndBlock { hash } => {
            let mut btc = Bitcoin::default();
            let querier = deps.querier;
            let storage = deps.storage;

            let pending_nbtc_transfers = btc.take_pending_completed(storage)?;

            let config = CONFIG.load(storage)?;
            let token_factory = config.token_factory_addr;

            let mut msgs = vec![];
            for pending in pending_nbtc_transfers {
                for (dest, coin) in pending {
                    // TODO: if dest is IBC packet, then forward to ibc bridge wasm
                    msgs.push(WasmMsg::Execute {
                        contract_addr: token_factory.to_string(),
                        msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                            denom: coin.denom.to_owned(),
                            amount: coin.amount,
                            mint_to_address: dest.to_source_addr(),
                        })?,
                        funds: vec![],
                    });
                }
            }

            let external_outputs: Vec<bitcoin::TxOut> =
                if btc.should_push_checkpoint(env.clone(), storage)? {
                    // TODO: build output
                    vec![]
                    // self.cosmos
                    //     .build_outputs(&self.ibc, btc.checkpoints.index)?
                } else {
                    vec![]
                };

            let offline_signers = btc.begin_block_step(
                env,
                storage,
                external_outputs.into_iter().map(Ok),
                hash.to_vec(),
            )?;

            for cons_key in &offline_signers {
                let (_, address) = VALIDATORS.load(storage, cons_key)?;
                // punish_downtime(address)?;
                println!("need punish downtime for {}", address);
            }

            Ok(Response::new())
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositFees { index } => {
            to_json_binary(&query_deposit_fees(deps.storage, index)?)
        }
        QueryMsg::WithdrawalFees { address, index } => {
            to_json_binary(&query_withdrawal_fees(deps.storage, address, index)?)
        }
        QueryMsg::HeaderHeight {} => to_json_binary(&query_header_height(deps.storage)?),
        QueryMsg::SidechainBlockHash {} => {
            to_json_binary(&query_sidechain_block_hash(deps.storage)?)
        }
        QueryMsg::CheckpointByIndex { index } => {
            to_json_binary(&query_checkpoint_by_index(deps.storage, index)?)
        }
        QueryMsg::SigningRecoveryTxs { xpub } => to_json_binary(&query_signing_recovery_txs(
            deps.querier,
            deps.storage,
            xpub,
        )?),
        QueryMsg::SigningTxsAtCheckpointIndex {
            xpub,
            checkpoint_index,
        } => to_json_binary(&query_signing_txs_at_checkpoint_index(
            deps.querier,
            deps.storage,
            xpub,
            checkpoint_index,
        )?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let original_version =
        cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("new_version", original_version.to_string()))
}
