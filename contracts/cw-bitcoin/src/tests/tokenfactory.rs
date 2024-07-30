use cosmwasm_std::{Addr, Coin, Uint128};
use token_bindings::{
    DenomsByCreatorResponse, FullDenomResponse, Metadata, TokenFactoryMsg, TokenFactoryMsgOptions,
    TokenFactoryQuery, TokenFactoryQueryEnum,
};

use crate::tests::helper::MockApp;

#[test]
fn mint_token() {
    let rcpt = Addr::unchecked("townies");
    let subdenom = "fundz";

    let mut app = MockApp::new(&[]);

    let contract = app.create_tokenfactory(Addr::unchecked("alice")).unwrap();

    // no tokens
    let start = app.as_querier().query_all_balances(rcpt.as_str()).unwrap();
    assert_eq!(start, vec![]);

    // let's find the mapping
    let FullDenomResponse { denom } = app
        .as_querier()
        .query(
            &TokenFactoryQuery::Token(TokenFactoryQueryEnum::FullDenom {
                creator_addr: contract.to_string(),
                subdenom: subdenom.to_string(),
            })
            .into(),
        )
        .unwrap();
    assert_ne!(denom, subdenom);
    assert!(denom.len() > 10);

    // prepare to mint
    let amount = Uint128::new(1234567);
    let msg = TokenFactoryMsg::Token(TokenFactoryMsgOptions::MintTokens {
        denom: denom.to_string(),
        amount,
        mint_to_address: rcpt.to_string(),
    });

    // fails to mint token before creating it
    let error = app
        .execute(Addr::unchecked("alice"), contract.clone(), &msg, &[])
        .unwrap_err();
    assert!(error
        .root_cause()
        .to_string()
        .contains("Token denom was never created"));

    // create the token now
    let create = TokenFactoryMsg::Token(TokenFactoryMsgOptions::CreateDenom {
        subdenom: subdenom.to_string(),
        metadata: Some(Metadata {
            description: Some("Awesome token, get it now!".to_string()),
            denom_units: vec![],
            base: None,
            display: Some("FUNDZ".to_string()),
            name: Some("Fundz pays".to_string()),
            symbol: Some("FUNDZ".to_string()),
        }),
    });
    app.execute(Addr::unchecked("alice"), contract.clone(), &create, &[])
        .unwrap();

    // now we can mint
    app.execute(Addr::unchecked("alice"), contract.clone(), &msg, &[])
        .unwrap();

    // we got tokens!
    let end = app
        .as_querier()
        .query_balance(rcpt.as_str(), &denom)
        .unwrap();
    let expected = Coin { denom, amount };
    assert_eq!(end, expected);

    println!("{:?}", end);

    // but no minting of unprefixed version
    let empty = app
        .as_querier()
        .query_balance(rcpt.as_str(), subdenom)
        .unwrap();
    assert_eq!(empty.amount, Uint128::zero());

    let DenomsByCreatorResponse { denoms } = app
        .as_querier()
        .query(
            &TokenFactoryQuery::Token(TokenFactoryQueryEnum::DenomsByCreator {
                creator: contract.to_string(),
            })
            .into(),
        )
        .unwrap();

    println!("{:?} {}", denoms, contract);
}
