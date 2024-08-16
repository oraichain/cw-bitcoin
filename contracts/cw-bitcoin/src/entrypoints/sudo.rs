use crate::{
    app::Bitcoin,
    error::ContractResult,
    fee::process_deduct_fee,
    state::{CONFIG, VALIDATORS},
};
use cosmwasm_std::{
    to_json_binary, Api, Binary, Env, QuerierWrapper, Response, Storage, Uint128, WasmMsg,
};

pub fn clock_end_block(
    env: &Env,
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    api: &dyn Api,
    hash: Binary,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();

    let pending_nbtc_transfers = btc.take_pending_completed(storage)?;

    let config = CONFIG.load(storage)?;
    let token_factory = config.token_factory_addr;

    let mut msgs = vec![];
    for pending in pending_nbtc_transfers {
        for (dest, coin) in pending {
            let fee_data = process_deduct_fee(storage, querier, api, coin.clone())?;
            msgs.push(WasmMsg::Execute {
                contract_addr: token_factory.to_string(),
                msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                    denom: coin.denom.to_owned(),
                    amount: fee_data.deducted_amount,
                    mint_to_address: dest.to_source_addr(),
                })?,
                funds: vec![],
            });

            if fee_data.relayer_fee.amount.gt(&Uint128::zero()) {
                msgs.push(WasmMsg::Execute {
                    contract_addr: token_factory.to_string(),
                    msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                        denom: coin.denom.to_owned(),
                        amount: fee_data.relayer_fee.amount,
                        mint_to_address: config.relayer_fee_receiver.to_string(),
                    })?,
                    funds: vec![],
                });
            }

            if fee_data.token_fee.amount.gt(&Uint128::zero()) {
                msgs.push(WasmMsg::Execute {
                    contract_addr: token_factory.to_string(),
                    msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                        denom: coin.denom.to_owned(),
                        amount: fee_data.token_fee.amount,
                        mint_to_address: config.token_fee_receiver.to_string(),
                    })?,
                    funds: vec![],
                });
            }
        }
    }

    let offline_signers = btc.begin_block_step(env.clone(), storage, hash.to_vec())?;

    for cons_key in &offline_signers {
        let (_, address) = VALIDATORS.load(storage, cons_key)?;
        btc.punish_validator(storage, cons_key, address)?;
    }

    Ok(Response::new().add_messages(msgs))
}
