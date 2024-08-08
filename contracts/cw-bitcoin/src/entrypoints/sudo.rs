use crate::{
    app::Bitcoin,
    error::ContractResult,
    state::{CONFIG, VALIDATORS},
};
use cosmwasm_std::{to_json_binary, Binary, Env, Response, Storage, WasmMsg};

pub fn clock_end_block(
    env: &Env,
    storage: &mut dyn Storage,
    hash: Binary,
) -> ContractResult<Response> {
    let mut btc = Bitcoin::default();

    let pending_nbtc_transfers = btc.take_pending_completed(storage)?;

    let config = CONFIG.load(storage)?;
    let token_factory = config.token_factory_addr;

    let mut msgs = vec![];
    for pending in pending_nbtc_transfers {
        for (dest, coin) in pending {
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

    let offline_signers = btc.begin_block_step(env.clone(), storage, hash.to_vec())?;

    for cons_key in &offline_signers {
        let (_, address) = VALIDATORS.load(storage, cons_key)?;
        btc.punish_validator(storage, cons_key, address)?;
    }

    Ok(Response::new().add_messages(msgs))
}
