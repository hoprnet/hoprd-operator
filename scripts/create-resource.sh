#!/usr/bin/env bash

set -Eeuo pipefail

set -x
safe_address=$(grep "Logs" -A 3 "/app/hoprd-identity-created/create-safe-module.log" | grep safeAddress | cut -d ' ' -f 4)
module_address=$(grep "Logs" -A 3 "/app/hoprd-identity-created/create-safe-module.log" | grep safeAddress | cut -d ' ' -f 6)
peer_id=$(jq -r '.".hoprd0.id".peer_id' /app/hoprd-identity-created/hoprd.json)
native_address=$(jq -r '.".hoprd0.id".native_address' /app/hoprd-identity-created/hoprd.json)
identity_file=$(cat /app/hoprd-identity-created/.hoprd0.id | base64 | tr -d '\n')
cat <<EOF > "/app/hoprd-identity-created/identityHorpd.yaml"
---
apiVersion: hoprnet.org/v1alpha2
kind: IdentityHoprd
metadata:
  namespace: ${JOB_NAMESPACE}
  name: ${IDENTITY_NAME}
spec:
  identityPoolName: ${IDENTITY_POOL_NAME}
  identityFile: ${identity_file}
  peerId: "${peer_id}"
  nativeAddress: "${native_address}"
  safeAddress: "${safe_address}"
  moduleAddress: "${module_address}"
EOF
kubectl apply -f  /app/hoprd-identity-created/identityHorpd.yaml