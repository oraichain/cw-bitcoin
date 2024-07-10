import * as wasm from './pkg/wasm';

const signatorySet = wasm.newSignatorySet(
  '2103f8cd1e3bc96a4bda589dfebf67397e8d5413623dc4f5743298e20792e78caa33ac6303e5ec116700687c2102f0a4fd5299322d99279ceb95ceec12d7807ae6fd009e350a34c5c0044448562bac630325cd0e93687c210333133305671d936ea737e80f39d62aa1eed159c77a8c2efe6bffa032a7195dc2ac6303fa940893687c2103ad7e43d72661b1d9234fce42cc7f0a79bcf42b7e4e39e7f4d227de11fced69abac6303d0fa0693687c2103126522ef0dfa543b3097a74d23b3f9e8de7bb31bd02d833050a341d5a6086139ac6303b8680693687c210269efe3541f938a1e8e759b03e86ca0f995fced28ee076e88b396190093834f83ac6303ce610693687c2103f1328d145b98e0fdc814a7820d6a90d7d47f14445ff8693e9061b217b427bb4bac630377340593687c2102902d0e76a94201d4bed3dbf5e233eb6ad94dcaa6d69ee4665c736ab9421fba88ac630315ac0493687c21031e1f44ef95e582f727a29e318160dbb468df566ab55b2549bb8d570b1414fd28ac6303b8520493687c2102275211cbb6820da2268c2083d10553b350f7977a11cee7dc5f53dfc4f9f0b3aeac63038da60393687c21025c2a8e5368369a0b56d5d522b40f822e58966917a36ab73a46b9e0a5a122c6f9ac6303415e0393687c2103100e355b6d064e6d608fa8a6abd17e3bedc1498fb569b62a04b4d897d0a74b6eac6303234f0393687c210249c893574b02aab8f273df0bcc0b524634ec1811576c6e1f61e2527f9d8835ddac63039f290393687c210394157139902ea6929069280a7fdbc82fee003801e8e818494c5fce23d340a557ac6303c50f0393687c2103a5cd2a017b57002f1f91f44a65d40ac2761e3978b0563b1adb4d780ff6935240ac6303a3fb0293687c21030c075275a90ec9e214a4700816c94c94c470f7707cf34c2dd78bc0d870941878ac6303e4c80293687c21034af380a1962705a6c829f039386d8b600691cced67780072ff22951ab6e29732ac63037bc80293687c2102294e8a80e2e3f58d32a8d074f0a37679553d45db42975df6eca2d853bdfa1a2fac63037fc70293687c2102fb69dfafbeb36a5c50de86b3fcbb24945540eccef2da3414ecc72f02047e2b70ac63036cb90293687c2103f9981616d2e06a7182bc70be5f40f0e0e685974e3a8b0606dbf7e8b2d4821e24ac6303c9b402936803226548a0010075',
  2n,
  3n
);
console.log(signatorySet);
