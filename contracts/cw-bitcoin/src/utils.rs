use bitcoin::secp256k1;
use cosmwasm_std::StdError;
use libsecp256k1_core::curve::{Affine, ECMultContext, Field, Scalar};
use libsecp256k1_core::util::{TAG_PUBKEY_EVEN, TAG_PUBKEY_ODD};

use crate::error::ContractResult;

pub fn parse_pubkey(public_key: &secp256k1::PublicKey) -> ContractResult<Affine> {
    let bytes = public_key.serialize();
    let mut x = Field::default();
    if !x.set_b32(arrayref::array_ref!(&bytes, 1, 32)) {
        return Err(StdError::generic_err("invalid pubkey").into());
    }
    let mut elem = libsecp256k1_core::curve::Affine::default();
    elem.set_xo_var(&x, bytes[0] == TAG_PUBKEY_ODD);
    Ok(elem)
}

pub fn add_exp_tweak(
    public_key: &secp256k1::PublicKey,
    secret: &secp256k1::SecretKey,
) -> ContractResult<secp256k1::PublicKey> {
    let tweak = secret.secret_bytes();
    let mut elem = parse_pubkey(public_key)?;
    let mut scala = Scalar::default();
    if bool::from(scala.set_b32(&tweak)) {
        return Err(StdError::generic_err("invalid secret").into());
    }

    let ctx = ECMultContext::new_boxed();
    let mut r = libsecp256k1_core::curve::Jacobian::default();
    let a = libsecp256k1_core::curve::Jacobian::from_ge(&elem);
    let one = libsecp256k1_core::curve::Scalar::from_int(1);
    ctx.ecmult(&mut r, &a, &one, &scala);

    elem.set_gej(&r);

    let mut ret = [0u8; 33];

    elem.x.normalize_var();
    elem.y.normalize_var();
    elem.x.fill_b32(arrayref::array_mut_ref!(ret, 1, 32));
    ret[0] = if elem.y.is_odd() {
        TAG_PUBKEY_ODD
    } else {
        TAG_PUBKEY_EVEN
    };
    let pubkey = secp256k1::PublicKey::from_slice(&ret)?;
    Ok(pubkey)
}
