#!/usr/bin/env bash

set -Eeuo pipefail

export PATH=${PATH}:/app/hoprnet/.foundry/bin/
export RUST_BACKTRACE=full
export PRIVATE_KEY=${DEPLOYER_PRIVATE_KEY}

set -x

/bin/hopli identity --action create --identity-directory /app/hoprd-identity-created --identity-prefix .hoprd | tee /app/hoprd-identity-created/hoprd.json
/bin/hopli create-safe-module --contracts-root /app/hoprnet/packages/ethereum/contracts/ --network "${HOPRD_NETWORK}" --identity-from-path /app/hoprd-identity-created/.hoprd0.id --hopr-amount "10" --native-amount "0.01" | tee /app/hoprd-identity-created/create-safe-module.log

# Download the next script here because the kubectl image does not have curl or wget
curl -s ${JOB_SCRIPT_URL} > /app/hoprd-identity-created/create-resource.sh
chmod +x /app/hoprd-identity-created/create-resource.sh

