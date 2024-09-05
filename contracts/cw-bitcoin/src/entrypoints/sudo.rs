use crate::{
    app::Bitcoin,
    fee::process_deduct_fee,
    state::{BLOCK_HASHES, CONFIG, VALIDATORS},
};
use common_bitcoin::error::{ContractError, ContractResult};
use cosmwasm_std::{
    to_json_binary, Api, Binary, Coin, CosmosMsg, Env, QuerierWrapper, Response, Storage, Uint128,
    WasmMsg,
};

pub fn clock_end_block(
    env: &Env,
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    api: &dyn Api,
    hash: Binary,
) -> ContractResult<Response> {
    if BLOCK_HASHES.has(storage, &hash) {
        return Err(ContractError::App("Blockhash already exists".to_string()));
    }

    let mut btc = Bitcoin::default();

    let pending_nbtc_transfers = btc.take_pending_completed(storage)?;

    let config = CONFIG.load(storage)?;
    let token_factory = config.token_factory_addr;
    let osor_entry_point_contract = config.osor_entry_point_contract;

    let mut msgs = vec![];
    for pending in pending_nbtc_transfers {
        for (dest, coin) in pending {
            let fee_data = process_deduct_fee(storage, querier, api, coin.clone())?;
            let denom = coin.denom.to_owned();

            dest.build_cosmos_msg(
                env,
                &mut msgs,
                Coin {
                    denom: denom.clone(),
                    amount: fee_data.deducted_amount,
                },
                env.contract.address.clone(),
                token_factory.clone(),
                osor_entry_point_contract.clone(),
            );

            if fee_data.relayer_fee.amount.gt(&Uint128::zero()) {
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: token_factory.to_string(),
                    msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                        denom: denom.clone(),
                        amount: fee_data.relayer_fee.amount,
                        mint_to_address: config.relayer_fee_receiver.to_string(),
                    })?,
                    funds: vec![],
                }));
            }

            if fee_data.token_fee.amount.gt(&Uint128::zero()) {
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: token_factory.to_string(),
                    msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                        denom: denom.clone(),
                        amount: fee_data.token_fee.amount,
                        mint_to_address: config.token_fee_receiver.to_string(),
                    })?,
                    funds: vec![],
                }));
            }
        }
    }
    let offline_signers = btc.begin_block_step(env, storage, hash.to_vec())?;
    for cons_key in &offline_signers {
        let (_, address) = VALIDATORS.load(storage, cons_key)?;
        btc.punish_validator(storage, cons_key, address)?;
    }
    BLOCK_HASHES.save(storage, &hash, &()).unwrap();

    Ok(Response::new().add_messages(msgs))
}
