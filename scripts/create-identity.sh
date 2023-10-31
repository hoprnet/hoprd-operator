    #!/usr/bin/env bash

    set -Eeuo pipefail

    export PATH=${PATH}:/root/.foundry/bin/
    export RUST_BACKTRACE=full
    export PRIVATE_KEY=${DEPLOYER_PRIVATE_KEY}

    set -x
    sleep 99999999
    /bin/hopli identity --action create --identity-directory "/app/hoprd-identity-created" --identity-prefix .hoprd
    mv "/app/hoprd-identity-created/.hoprd0.id" "/app/hoprd-identity-created/.hoprd.id"
    /bin/hopli identity --action read --identity-directory "/app/hoprd-identity-created" | tee "/app/hoprd-identity-created/hoprd.json"
    /bin/hopli create-safe-module --network "${HOPRD_NETWORK}" --identity-from-path "/app/hoprd-identity-created/.hoprd.id" --hopr-amount "10" --native-amount "0.01" | tee "/app/hoprd-identity-created/safe.log"
