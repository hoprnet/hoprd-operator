# Login into GCP Artifact Registry for Helm charts
login:
  #!/usr/bin/env bash
  token=$(gcloud auth print-access-token)
  helm registry login -u oauth2accesstoken --password "$token" https://europe-west3-docker.pkg.dev

# Start a debugging session by opening a port-forward to the hoprd-operator pod
debug:
  #!/usr/bin/env bash
  set -euo pipefail
  DISABLE_SYNC='[{"op": "remove", "path": "/spec/syncPolicy/automated"}]'
  kubectl patch Applications -n argocd hoprd-operator --type=json -p "${DISABLE_SYNC}" 2>/dev/null || true
  kubectl scale -n hoprd-operator deployment hoprd-operator-controller --replicas=0
  openssl req -x509 -nodes -days 365 -newkey rsa:2048 -keyout ./test-data/tls.key -out ./test-data/tls.crt -config ./test-data/tls.conf
  export CA_BUNDLE=$(cat ./test-data/tls.crt | base64 | tr -d '\n')
  WEBHOOK_CLIENT_CONFIG="[{
      \"op\": \"replace\",
      \"path\": \"/spec/conversion/webhook/clientConfig\",
      \"value\": {
        \"url\": \"https://malilla.duckdns.org:8443/convert\",
        \"caBundle\": \"${CA_BUNDLE}\"
      }
  }]"
  kubectl patch crd identitypools.hoprnet.org --type='json' -p "${WEBHOOK_CLIENT_CONFIG}"
  kubectl patch crd identityhoprds.hoprnet.org --type='json' -p "${WEBHOOK_CLIENT_CONFIG}"
  kubectl patch crd clusterhoprds.hoprnet.org --type='json' -p "${WEBHOOK_CLIENT_CONFIG}"
  kubectl patch crd hoprds.hoprnet.org --type='json' -p "${WEBHOOK_CLIENT_CONFIG}"
  

# Template the Helm chart for a given chart name
template chartName:
  #!/usr/bin/env bash
  set -euo pipefail
  if [ "{{ chartName }}" = "hoprd-operator" ] || [ "{{ chartName }}" = "hoprd-crds" ]; then
    namespace="hoprd-operator"
  else
    namespace="hoprd-operator-sample"
  fi
  helm template --dry-run --namespace $namespace --create-namespace -f ./charts/{{ chartName }}/values-staging.yaml -f ./charts/{{ chartName }}/secrets-staging.yaml {{ chartName }} ./charts/{{ chartName }}/

# Lint both Helm charts
lint:
  helm lint -f ./charts/hoprd-operator/values-staging.yaml ./charts/hoprd-operator
  helm lint -f ./charts/cluster-hoprd/values-staging.yaml ./charts/cluster-hoprd

# Generate README.md files for both Helm charts
docs:
  #npm install -g @bitnami/readme-generator-for-helm@2.7.2
  readme-generator --values ./charts/hoprd-operator/values.yaml --readme ./charts/hoprd-operator/README.md --schema "/tmp/schema.json"
  readme-generator --values ./charts/cluster-hoprd/values.yaml --readme ./charts/cluster-hoprd/README.md --schema "/tmp/schema.json"

# Build the Rust project
build:
  cargo build

# Run the Rust project in the background
run:
  #!/usr/bin/env bash
  export OPERATOR_INSTANCE_NAME="hoprd-operator"
  export OPERATOR_INSTANCE_NAMESPACE="hoprd-operator"
  export OPERATOR_ENVIRONMENT="staging"
  export RUST_BACKTRACE="full"
  export RUST_LOG="hoprd_operator=DEBUG"
  cargo run

# Package a Helm chart for a given chart name
package chartName:
  #!/usr/bin/env bash
  set -euo pipefail
  version=$(yq '.version' ./charts/{{ chartName }}/Chart.yaml )
  helm package ./charts/{{ chartName }}

# Push a Helm chart for a given chart name
push chartName:
  #!/usr/bin/env bash
  set -euo pipefail
  version=$(yq '.version' ./charts/{{ chartName }}/Chart.yaml )
  helm push {{ chartName }}-${version}.tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

# Builds docker image
docker-build: ## Builds docker image
  docker build -t europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest --platform linux/amd64 --progress plain .

# Deploys docker image into GCP Artifact registry
docker-push: ## Deploys docker image into GCP Artifact registry
  docker push europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest

# Builds Metrics docker image
docker-metrics-build: ## Builds Metrics docker image
  docker build -t europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator-metrics:latest --platform linux/amd64 --progress plain ./metrics-container

# Deploys Metrics docker image into GCP Artifact registry
docker-metrics-push: ## Deploys Metrics docker image into GCP Artifact registry
  docker push europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator-metrics:latest