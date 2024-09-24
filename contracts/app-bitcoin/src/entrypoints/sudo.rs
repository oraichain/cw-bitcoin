use crate::{
    app::Bitcoin,
    constants::VALIDATOR_ADDRESS_PREFIX,
    fee::process_deduct_fee,
    helper::{convert_addr_by_prefix, fetch_staking_validator},
    state::{BLOCK_HASHES, CONFIG, SIGNERS, VALIDATORS},
};
use common_bitcoin::{
    error::{ContractError, ContractResult},
    msg::BondStatus,
};
use cosmwasm_std::{
    wasm_execute, Api, Binary, Coin, Env, Order, QuerierWrapper, Response, Storage, Uint128,
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
                token_factory.as_str(),
                osor_entry_point_contract.clone(),
            );

            if !fee_data.relayer_fee.amount.is_zero() {
                msgs.push(
                    wasm_execute(
                        token_factory.as_str(),
                        &tokenfactory::msg::ExecuteMsg::MintTokens {
                            denom: denom.clone(),
                            amount: fee_data.relayer_fee.amount,
                            mint_to_address: config.relayer_fee_receiver.to_string(),
                        },
                        vec![],
                    )?
                    .into(),
                );
            }

            if !fee_data.token_fee.amount.is_zero() {
                msgs.push(
                    wasm_execute(
                        token_factory.as_str(),
                        &tokenfactory::msg::ExecuteMsg::MintTokens {
                            denom: denom.clone(),
                            amount: fee_data.token_fee.amount,
                            mint_to_address: config.token_fee_receiver.to_string(),
                        },
                        vec![],
                    )?
                    .into(),
                );
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

    #[cfg(feature = "native-validator")]
    {
        for signer in signer_addrs.iter() {
            let (addr, cons_key) = signer;
            let val_addr = convert_addr_by_prefix(
                String::from_utf8(addr.clone()).unwrap().as_str(),
                VALIDATOR_ADDRESS_PREFIX,
            );
            let binary_validator_result = fetch_staking_validator(querier, val_addr)?;
            let validator_response =
                QueryValidatorResponse::decode(binary_validator_result.as_slice()).unwrap();
            let validator_info = validator_response.validator;
            if validator_info.is_none() {
                // delete signers and validators
                SIGNERS.remove(storage, String::from_utf8(addr.clone()).unwrap().as_str());
                VALIDATORS.remove(storage, &cons_key);
                continue;
            }
            if let Some(validator) = validator_info {
                if validator.jailed || validator.status != BondStatus::Bonded as i32 {
                    // delete signers and validators
                    SIGNERS.remove(storage, String::from_utf8(addr.clone()).unwrap().as_str());
                    VALIDATORS.remove(storage, &cons_key);
                    continue;
                }

                let voting_power: u64 =
                    validator.tokens.parse().expect("Cannot parse voting power");
                VALIDATORS
                    .save(
                        storage,
                        &cons_key,
                        &(voting_power, String::from_utf8(addr.clone()).unwrap()),
                    )
                    .unwrap();
            }
        }
    }

    Ok(Response::new().add_messages(msgs))
}
