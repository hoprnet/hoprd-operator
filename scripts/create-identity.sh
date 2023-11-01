#!/usr/bin/env bash

set -Eeuo pipefail

export PATH=${PATH}:/root/.foundry/bin/
export RUST_BACKTRACE=full
export PRIVATE_KEY=${DEPLOYER_PRIVATE_KEY}

set -x
/bin/hopli identity --action create --identity-directory /app/hoprd-identity-created --identity-prefix .hoprd | tee /app/hoprd-identity-created/hoprd.json
/bin/hopli create-safe-module --contracts-root /root/contracts --network "${HOPRD_NETWORK}" --identity-from-path /app/hoprd-identity-created/.hoprd0.id --hopr-amount "10" --native-amount "0.01" | tee "/app/hoprd-identity-created/create-safe-module.log"
