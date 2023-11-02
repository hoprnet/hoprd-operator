#!/usr/bin/env bash

set -Eeuo pipefail

export PATH=${PATH}:/app/hoprnet/.foundry/bin/
export RUST_BACKTRACE=full
export PRIVATE_KEY=${DEPLOYER_PRIVATE_KEY}

set -x

#/bin/hopli identity --action create --identity-directory /app/hoprd-identity-created --identity-prefix .hoprd | tee /app/hoprd-identity-created/hoprd.json
#/bin/hopli create-safe-module --contracts-root /app/hoprnet/packages/ethereum/contracts/ --network "${HOPRD_NETWORK}" --identity-from-path /app/hoprd-identity-created/.hoprd0.id --hopr-amount "10" --native-amount "0.01" | tee /app/hoprd-identity-created/create-safe-module.log

# Download the next script here because the kubectl image does not have curl or wget
curl -s ${JOB_SCRIPT_URL} > /app/hoprd-identity-created/create-resource.sh


echo "{\"crypto\":{\"cipher\":\"aes-128-ctr\",\"cipherparams\":{\"iv\":\"0c8e445f7b57afa284f1e824ae9a930c\"},\"ciphertext\":\"f565dd77db497ff7bc9d9a8c81c76ad9f25e2a3cfff1464ed233275f30e06c26a1cfac6a018765aabe3bc392a9f4910f941f00d35eb7aa2476fb4fdafef1025a284c4cbf1b834f4ba53a6b65df6b44f196e0c72906aa967ffafcec479f7a4bd7aed87fd946eeb78d9e236ae10746ae153bdf47ae469bcb0f3c3262eacf440d9d36ce97e15bd768eb95e71f93afde10e69c1c350037a20c46be0a6edc52ae4210f3a70787ed0f2735346a35f7\",\"kdf\":\"scrypt\",\"kdfparams\":{\"dklen\":32,\"n\":8192,\"p\":1,\"r\":8,\"salt\":\"f3b68cc82430f45344ea95d0b339dc08987196c3fe6e9bd2c6649fd0de2f0ca0\"},\"mac\":\"273f15cdbe8869bf844e8845b07cd17a15e2c17c511e8b364f77b7e30e2dd1a8\"},\"id\":\"4ae9d885-d0b0-4eb0-a6f0-fc0fe70e6f31\",\"version\":3}" > /app/hoprd-identity-created/.hoprd0.id
echo "{ \".hoprd.id\": { \"peer_id\": \"12D3KooWGh8WFFuXEZvDSDrv17CYieUxeMrbvwow2WbgUyMB4dqW\", \"packet_key\": \"0x66274b0cbf82e01dbc9b3b2adde13fc6677dcf083c0b57d8c932d9b6a5f1b625\", \"chain_key\": \"0x0310668dd2baa587858b4d98d6b9c6529207511e8c9cce5f814b80f5c01ce70c7c\", \"native_address\": \"0x915ab25afe89849084c021377dba14181e6839f4\", \"uuid\": \"4ae9d885-d0b0-4eb0-a6f0-fc0fe70e6f31\" }, \"safe_address\": \"0x0D4Ec909963D2866B544cd0F6a7099C48a2dcE2f\", \"module_address\": \"0x7F699cB986Fe36314b23139e88583fB79831aEc6\" }" > /app/hoprd-identity-created/hoprd.json
echo "Successfully changed working directory to /Users/ausias/Documents/github/hoprnet/scripts/../packages/ethereum/contracts!
No files changed, compilation skipped
Script ran successfully.

== Return ==
safe: address 0x40adE876991138A4A7A49cf9009158bbD8A8A66d
module: address 0xf15dFA85A487C695662E7f2765b70785be263345

== Logs ==
  msgSender address: 0x158ec0eab714a9c67ff6f2f85da13c5d8d92849e
  --safeAddress 0x40adE876991138A4A7A49cf9009158bbD8A8A66d --moduleAddress 0xf15dFA85A487C695662E7f2765b70785be263345
  deployerAddress: 0x4ff4e61052a4dfb1be72866ab711ae08dd861976
  Nodes registered to Network Registry
  Manager set eligibility on network regsitry
  msgSender address: 0x158ec0eab714a9c67ff6f2f85da13c5d8d92849e
  msgSender address: 0x158ec0eab714a9c67ff6f2f85da13c5d8d92849e

==========================

Chain 100

Estimated gas price: 3.000000016 gwei

Estimated total gas used for script: 1902225

Estimated amount required: 0.0057066750304356 ETH

==========================
" > /app/hoprd-identity-created/create-safe-module.log