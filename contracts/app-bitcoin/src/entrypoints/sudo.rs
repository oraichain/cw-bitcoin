use crate::{
    app::Bitcoin,
    fee::process_deduct_fee,
    helper::fetch_staking_validator,
    state::{BLOCK_HASHES, CONFIG, SIGNERS, VALIDATORS},
};
use common_bitcoin::{
    error::{ContractError, ContractResult},
    msg::BondStatus,
};
use cosmwasm_std::{
    to_json_binary, Api, Binary, Coin, CosmosMsg, Env, Order, QuerierWrapper, Response, Storage,
    Uint128, WasmMsg,
};
use ibc_proto::cosmos::staking::v1beta1::QueryValidatorResponse;
use prost::Message;

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
    let token_factory = config.token_factory_contract;
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
    let offline_signers = btc.begin_block_step(env, querier, storage, hash.to_vec())?;
    for cons_key in &offline_signers {
        let (_, address) = VALIDATORS.load(storage, cons_key)?;
        btc.punish_validator(storage, cons_key, address)?;
    }
    BLOCK_HASHES.save(storage, &hash, &()).unwrap();

    let mut signer_addrs = Vec::new();
    for signer in SIGNERS.range_raw(storage, None, None, Order::Ascending) {
        signer_addrs.push(signer.unwrap());
    }

    // for signer in signer_addrs.iter() {
    //     let (addr, cons_key) = signer;
    //     let binary_validator_result =
    //         fetch_staking_validator(querier, String::from_utf8(addr.clone()).unwrap())?;
    //     let validator_response =
    //         QueryValidatorResponse::decode(binary_validator_result.as_slice()).unwrap();
    //     let validator_info = validator_response.validator;
    //     if validator_info.is_none() {
    //         // delete signers and validators
    //         SIGNERS.remove(storage, String::from_utf8(addr.clone()).unwrap().as_str());
    //         VALIDATORS.remove(storage, &cons_key);
    //         continue;
    //     }
    //     if let Some(validator) = validator_info {
    //         if validator.jailed || validator.status != BondStatus::Bonded as i32 {
    //             // delete signers and validators
    //             SIGNERS.remove(storage, String::from_utf8(addr.clone()).unwrap().as_str());
    //             VALIDATORS.remove(storage, &cons_key);
    //             continue;
    //         }

    //         let voting_power: u64 = validator.tokens.parse().expect("Cannot parse voting power");
    //         VALIDATORS
    //             .save(
    //                 storage,
    //                 &cons_key,
    //                 &(voting_power, String::from_utf8(addr.clone()).unwrap()),
    //             )
    //             .unwrap();
    //     }
    // }

    Ok(Response::new().add_messages(msgs))
}
