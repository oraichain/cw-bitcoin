use bitcoin::{hashes::hex::FromHex, Script};

use crate::{
    signatory::{Signatory, SignatorySet},
    threshold_sig::Pubkey,
};
use common_bitcoin::error::ContractResult;

fn mock_signatory_set() -> SignatorySet {
    let pk = |bytes| Pubkey::new(bytes).unwrap().into();
    let sigsets = SignatorySet {
        create_time: 0,
        present_vp: 12000,
        possible_vp: 12000,
        index: 25,
        signatories: vec![
            Signatory {
                voting_power: 3000,
                pubkey: pk([
                    3, 144, 133, 164, 227, 11, 15, 32, 57, 222, 17, 218, 214, 79, 52, 149, 38, 2,
                    111, 79, 87, 97, 245, 111, 169, 180, 238, 39, 224, 105, 176, 180, 137,
                ]),
            },
            Signatory {
                voting_power: 4000,
                pubkey: pk([
                    3, 119, 35, 166, 180, 67, 152, 140, 51, 249, 141, 208, 60, 212, 134, 142, 244,
                    196, 239, 254, 145, 69, 173, 168, 87, 3, 61, 4, 39, 64, 235, 183, 78,
                ]),
            },
            Signatory {
                voting_power: 5000,
                pubkey: pk([
                    2, 42, 187, 47, 134, 233, 86, 222, 154, 32, 48, 177, 77, 19, 39, 220, 123, 106,
                    30, 198, 20, 232, 248, 204, 95, 82, 54, 94, 0, 40, 251, 133, 106,
                ]),
            },
        ],
        foundation_signatories: vec![],
    };
    sigsets
}

#[test]
fn test_redeem_script_creation() {
    let sigsets = mock_signatory_set();
    let script = sigsets.redeem_script(
        &[
            19, 147, 69, 143, 37, 59, 136, 7, 10, 113, 245, 63, 123, 14, 25, 150, 162, 72, 106, 16,
            123, 127, 34, 111, 110, 139, 181, 102, 8, 1, 206, 250,
        ],
        (2, 3),
    );

    let into_script_result: ContractResult<Script> = script.into();
    let final_script = into_script_result.unwrap();
    let script_gen_by_js_lib = [
        1, 0, 135, 99, 33, 3, 144, 133, 164, 227, 11, 15, 32, 57, 222, 17, 218, 214, 79, 52, 149,
        38, 2, 111, 79, 87, 97, 245, 111, 169, 180, 238, 39, 224, 105, 176, 180, 137, 172, 99, 2,
        184, 11, 103, 0, 104, 124, 33, 3, 119, 35, 166, 180, 67, 152, 140, 51, 249, 141, 208, 60,
        212, 134, 142, 244, 196, 239, 254, 145, 69, 173, 168, 87, 3, 61, 4, 39, 64, 235, 183, 78,
        172, 99, 2, 160, 15, 147, 104, 124, 33, 2, 42, 187, 47, 134, 233, 86, 222, 154, 32, 48,
        177, 77, 19, 39, 220, 123, 106, 30, 198, 20, 232, 248, 204, 95, 82, 54, 94, 0, 40, 251,
        133, 106, 172, 99, 2, 136, 19, 147, 104, 2, 64, 31, 160, 32, 19, 147, 69, 143, 37, 59, 136,
        7, 10, 113, 245, 63, 123, 14, 25, 150, 162, 72, 106, 16, 123, 127, 34, 111, 110, 139, 181,
        102, 8, 1, 206, 250, 117, 103, 106, 104,
    ];
    assert_eq!(final_script.clone().into_bytes(), script_gen_by_js_lib);
}

#[test]
fn test_output_script() {
    let sigsets = mock_signatory_set();
    let output_script = sigsets
        .output_script(
            &[
                19, 147, 69, 143, 37, 59, 136, 7, 10, 113, 245, 63, 123, 14, 25, 150, 162, 72, 106,
                16, 123, 127, 34, 111, 110, 139, 181, 102, 8, 1, 206, 250,
            ],
            (2, 3),
        )
        .unwrap();
    let addr = bitcoin::Address::from_script(&output_script, bitcoin::Network::Bitcoin).unwrap();
    assert_eq!(
        addr.to_string(),
        "bc1qysvk4ytl4ddd5u8z5g6ld02el23n9avv6najqrcpan4lj7yz0naqe9uqdu".to_string()
    );
}

#[test]
fn from_script() {
    let script = bitcoin::Script::from_hex("0100876321028891f36b691a40036f2b3ecb17c13780a932503ef2c39f3faed9b95bf71ea27fac630339e0116700687c2102f6fee7ad7dc87d0a636ae1584273c849bf540f4c1780434a0430888b0c5b151cac63033c910e93687c2102d207371a1e9a588e447d91dc12a8f3479f1f9ff8da748aae04bb5d07f0737790ac630371730893687c2103713e9bb6025fa9dc3c26507762cffd2a9524ff48f1d84c6753caa581347e5e10ac63031def0793687c2103d8fc0412a866bfb14d3fbc9e1b714ca31141d0f7e211d0fa634d53dda9789ecaac6303d1f00693687c2102c7961e04206af92f4b4cf3f19b43722f301e4915a49f5ca2908d9af5ce343830ac6303496f0693687c2103205472bb87799cb9140b5d471cc045b65821a4e75591026a8411ee3ac3e27027ac6303fe500693687c2102c923df10e8141072504b1f9513ee6796dc4d748d774ce9396942b63d42d3d575ac6303ed1f0593687c21031e8124547a5f28e04652d61fab1053ba8af41b682ccecdf5fa58595add7c7d9eac6303d4a00493687c21038060738940b9b3513851aa45df9f8b9d8e3304ef5abc5f8c1928bf4f1c8601adac630347210493687c21022e1efe78c688bceb7a36bf8af0e905da65e1942b84afe31716a356a91c0d9c05ac6303c5620393687c21020598956ed409e190b763bed8ed1ec3a18138c582c761eb8a4cf60861bfb44f13ac6303b3550393687c2102c8b2e54cafced96b1438e9ee6ebddc27c4aca68f14b2199eb8b8da111b584c2cac63036c330393687c2102d8a4c0accefa93b6a8d390a81dbffa4d05cd0a844371b2bed0ba1b1b65e14300ac6303521d0393687c2102460ccc0db97b1027e4fe2ab178f015a786b6b8f016b580f495dde3230f34984cac630304060393687c2102def64dfc155e17988ea6dee5a5659e2ec0a19fce54af90ca84dcd4df53b1a222ac630341d20293687c21030c9057c92c19f749c891037379766c0642d03bd1c50e3b262fc7d954c232f4d8ac630356c30293687c21027e1ebe3dd4fbbf250a8161a8a7af19815d5c07363e220f28f81c535c3950c7cbac6303d3ab0293687c210235e1d72961cb475971e2bc437ac21f9be13c83f1aa039e64f406aae87e2b4816ac6303bdaa0293687c210295d565c8ae94d46d439b4591dcd146742f918893292c23c49d000c4023bad4ffac630308aa029368030fb34aa0010075676a68").unwrap();

    let (sigset, commitment) = SignatorySet::from_script(&script, (2, 3)).unwrap();

    let pk = |bytes| Pubkey::new(bytes).unwrap().into();
    assert_eq!(
        sigset,
        SignatorySet {
            create_time: 0,
            present_vp: 7343255,
            possible_vp: 7343255,
            index: 0,
            signatories: vec![
                Signatory {
                    voting_power: 1171513,
                    pubkey: pk([
                        2, 136, 145, 243, 107, 105, 26, 64, 3, 111, 43, 62, 203, 23, 193, 55, 128,
                        169, 50, 80, 62, 242, 195, 159, 63, 174, 217, 185, 91, 247, 30, 162, 127
                    ])
                },
                Signatory {
                    voting_power: 954684,
                    pubkey: pk([
                        2, 246, 254, 231, 173, 125, 200, 125, 10, 99, 106, 225, 88, 66, 115, 200,
                        73, 191, 84, 15, 76, 23, 128, 67, 74, 4, 48, 136, 139, 12, 91, 21, 28
                    ])
                },
                Signatory {
                    voting_power: 553841,
                    pubkey: pk([
                        2, 210, 7, 55, 26, 30, 154, 88, 142, 68, 125, 145, 220, 18, 168, 243, 71,
                        159, 31, 159, 248, 218, 116, 138, 174, 4, 187, 93, 7, 240, 115, 119, 144
                    ])
                },
                Signatory {
                    voting_power: 519965,
                    pubkey: pk([
                        3, 113, 62, 155, 182, 2, 95, 169, 220, 60, 38, 80, 119, 98, 207, 253, 42,
                        149, 36, 255, 72, 241, 216, 76, 103, 83, 202, 165, 129, 52, 126, 94, 16
                    ])
                },
                Signatory {
                    voting_power: 454865,
                    pubkey: pk([
                        3, 216, 252, 4, 18, 168, 102, 191, 177, 77, 63, 188, 158, 27, 113, 76, 163,
                        17, 65, 208, 247, 226, 17, 208, 250, 99, 77, 83, 221, 169, 120, 158, 202
                    ])
                },
                Signatory {
                    voting_power: 421705,
                    pubkey: pk([
                        2, 199, 150, 30, 4, 32, 106, 249, 47, 75, 76, 243, 241, 155, 67, 114, 47,
                        48, 30, 73, 21, 164, 159, 92, 162, 144, 141, 154, 245, 206, 52, 56, 48
                    ])
                },
                Signatory {
                    voting_power: 413950,
                    pubkey: pk([
                        3, 32, 84, 114, 187, 135, 121, 156, 185, 20, 11, 93, 71, 28, 192, 69, 182,
                        88, 33, 164, 231, 85, 145, 2, 106, 132, 17, 238, 58, 195, 226, 112, 39
                    ])
                },
                Signatory {
                    voting_power: 335853,
                    pubkey: pk([
                        2, 201, 35, 223, 16, 232, 20, 16, 114, 80, 75, 31, 149, 19, 238, 103, 150,
                        220, 77, 116, 141, 119, 76, 233, 57, 105, 66, 182, 61, 66, 211, 213, 117
                    ])
                },
                Signatory {
                    voting_power: 303316,
                    pubkey: pk([
                        3, 30, 129, 36, 84, 122, 95, 40, 224, 70, 82, 214, 31, 171, 16, 83, 186,
                        138, 244, 27, 104, 44, 206, 205, 245, 250, 88, 89, 90, 221, 124, 125, 158
                    ])
                },
                Signatory {
                    voting_power: 270663,
                    pubkey: pk([
                        3, 128, 96, 115, 137, 64, 185, 179, 81, 56, 81, 170, 69, 223, 159, 139,
                        157, 142, 51, 4, 239, 90, 188, 95, 140, 25, 40, 191, 79, 28, 134, 1, 173
                    ])
                },
                Signatory {
                    voting_power: 221893,
                    pubkey: pk([
                        2, 46, 30, 254, 120, 198, 136, 188, 235, 122, 54, 191, 138, 240, 233, 5,
                        218, 101, 225, 148, 43, 132, 175, 227, 23, 22, 163, 86, 169, 28, 13, 156,
                        5
                    ])
                },
                Signatory {
                    voting_power: 218547,
                    pubkey: pk([
                        2, 5, 152, 149, 110, 212, 9, 225, 144, 183, 99, 190, 216, 237, 30, 195,
                        161, 129, 56, 197, 130, 199, 97, 235, 138, 76, 246, 8, 97, 191, 180, 79,
                        19
                    ])
                },
                Signatory {
                    voting_power: 209772,
                    pubkey: pk([
                        2, 200, 178, 229, 76, 175, 206, 217, 107, 20, 56, 233, 238, 110, 189, 220,
                        39, 196, 172, 166, 143, 20, 178, 25, 158, 184, 184, 218, 17, 27, 88, 76,
                        44
                    ])
                },
                Signatory {
                    voting_power: 204114,
                    pubkey: pk([
                        2, 216, 164, 192, 172, 206, 250, 147, 182, 168, 211, 144, 168, 29, 191,
                        250, 77, 5, 205, 10, 132, 67, 113, 178, 190, 208, 186, 27, 27, 101, 225,
                        67, 0
                    ])
                },
                Signatory {
                    voting_power: 198148,
                    pubkey: pk([
                        2, 70, 12, 204, 13, 185, 123, 16, 39, 228, 254, 42, 177, 120, 240, 21, 167,
                        134, 182, 184, 240, 22, 181, 128, 244, 149, 221, 227, 35, 15, 52, 152, 76
                    ])
                },
                Signatory {
                    voting_power: 184897,
                    pubkey: pk([
                        2, 222, 246, 77, 252, 21, 94, 23, 152, 142, 166, 222, 229, 165, 101, 158,
                        46, 192, 161, 159, 206, 84, 175, 144, 202, 132, 220, 212, 223, 83, 177,
                        162, 34
                    ])
                },
                Signatory {
                    voting_power: 181078,
                    pubkey: pk([
                        3, 12, 144, 87, 201, 44, 25, 247, 73, 200, 145, 3, 115, 121, 118, 108, 6,
                        66, 208, 59, 209, 197, 14, 59, 38, 47, 199, 217, 84, 194, 50, 244, 216
                    ])
                },
                Signatory {
                    voting_power: 175059,
                    pubkey: pk([
                        2, 126, 30, 190, 61, 212, 251, 191, 37, 10, 129, 97, 168, 167, 175, 25,
                        129, 93, 92, 7, 54, 62, 34, 15, 40, 248, 28, 83, 92, 57, 80, 199, 203
                    ])
                },
                Signatory {
                    voting_power: 174781,
                    pubkey: pk([
                        2, 53, 225, 215, 41, 97, 203, 71, 89, 113, 226, 188, 67, 122, 194, 31, 155,
                        225, 60, 131, 241, 170, 3, 158, 100, 244, 6, 170, 232, 126, 43, 72, 22
                    ])
                },
                Signatory {
                    voting_power: 174600,
                    pubkey: pk([
                        2, 149, 213, 101, 200, 174, 148, 212, 109, 67, 155, 69, 145, 220, 209, 70,
                        116, 47, 145, 136, 147, 41, 44, 35, 196, 157, 0, 12, 64, 35, 186, 212, 255
                    ])
                }
            ],
            foundation_signatories: vec![]
        }
    );
    assert_eq!(commitment, vec![0]);
}
