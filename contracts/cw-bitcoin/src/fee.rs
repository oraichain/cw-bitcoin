use cosmwasm_std::{Api, Coin, Decimal, QuerierWrapper, StdResult, Storage, Uint128};

use oraiswap::{
    asset::AssetInfo,
    router::{RouterController, SwapOperation},
};
use std::ops::Mul;

use crate::{
    helper::denom_to_asset_info,
    msg::FeeData,
    state::{Ratio, CONFIG, TOKEN_FEE_RATIO},
};

pub fn process_deduct_fee(
    storage: &dyn Storage,
    querier: &QuerierWrapper,
    api: &dyn Api,
    local_amount: Coin, // local amount
) -> StdResult<FeeData> {
    let local_denom = local_amount.denom.clone();
    let (deducted_amount, token_fee) = deduct_token_fee(storage, local_amount.amount)?;

    let mut fee_data = FeeData {
        deducted_amount,
        token_fee: Coin {
            denom: local_denom.clone(),
            amount: token_fee,
        },
        relayer_fee: Coin {
            denom: local_denom.clone(),
            amount: Uint128::from(0u64),
        },
    };
    // if after token fee, the deducted amount is 0 then we deduct all to token fee
    if deducted_amount.is_zero() {
        fee_data.token_fee = local_amount;
        return Ok(fee_data);
    }

    // simulate for relayer fee
    let ask_asset_info = denom_to_asset_info(api, &local_amount.denom);
    let relayer_fee = deduct_relayer_fee(storage, querier, ask_asset_info)?;

    fee_data.deducted_amount = deducted_amount.checked_sub(relayer_fee).unwrap_or_default();
    fee_data.relayer_fee = Coin {
        denom: local_denom.clone(),
        amount: relayer_fee,
    };
    // if the relayer fee makes the final amount 0, then we charge the remaining deducted amount as relayer fee
    if fee_data.deducted_amount.is_zero() {
        fee_data.relayer_fee = Coin {
            denom: local_denom.clone(),
            amount: deducted_amount,
        };
        return Ok(fee_data);
    }
    Ok(fee_data)
}

pub fn deduct_relayer_fee(
    storage: &dyn Storage,
    querier: &QuerierWrapper,
    ask_asset_info: AssetInfo,
) -> StdResult<Uint128> {
    let config = CONFIG.load(storage)?;

    // no need to deduct fee if no fee is found in the mapping
    if config.relayer_fee.is_zero() {
        return Ok(Uint128::from(0u64));
    }

    if config.swap_router_contract.is_none() {
        return Ok(Uint128::from(0u64));
    }

    let relayer_fee = get_swap_token_amount_out(
        querier,
        config.relayer_fee,
        &RouterController(config.swap_router_contract.unwrap().to_string()),
        ask_asset_info,
        config.relayer_fee_token,
    );

    Ok(relayer_fee)
}

pub fn deduct_token_fee(storage: &dyn Storage, amount: Uint128) -> StdResult<(Uint128, Uint128)> {
    let token_fee = TOKEN_FEE_RATIO.may_load(storage)?;
    if let Some(token_fee) = token_fee {
        let fee = deduct_fee(token_fee, amount);
        let new_deducted_amount = amount.checked_sub(fee)?;
        return Ok((new_deducted_amount, fee));
    }
    Ok((amount, Uint128::from(0u64)))
}

pub fn deduct_fee(token_fee: Ratio, amount: Uint128) -> Uint128 {
    // ignore case where denominator is zero since we cannot divide with 0
    if token_fee.denominator == 0 {
        return Uint128::from(0u64);
    }

    amount.mul(Decimal::from_ratio(
        token_fee.nominator,
        token_fee.denominator,
    ))
}

pub fn get_swap_token_amount_out(
    querier: &QuerierWrapper,
    offer_amount: Uint128,
    swap_router_contract: &RouterController,
    ask_asset_info: AssetInfo,
    relayer_fee_token: AssetInfo,
) -> Uint128 {
    if ask_asset_info.eq(&relayer_fee_token) {
        return offer_amount;
    }

    let orai_asset = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };

    let swap_ops = if ask_asset_info.eq(&orai_asset) || relayer_fee_token.eq(&orai_asset) {
        vec![SwapOperation::OraiSwap {
            offer_asset_info: relayer_fee_token,
            ask_asset_info,
        }]
    } else {
        vec![
            SwapOperation::OraiSwap {
                offer_asset_info: relayer_fee_token,
                ask_asset_info: orai_asset.clone(),
            },
            SwapOperation::OraiSwap {
                offer_asset_info: orai_asset,
                ask_asset_info,
            },
        ]
    };

    swap_router_contract
        .simulate_swap(querier, offer_amount, swap_ops)
        .map(|data| data.amount)
        .unwrap_or_default()
}
