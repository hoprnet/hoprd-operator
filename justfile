# Login into GCP Artifact Registry for Helm charts
login:
  #!/usr/bin/env bash
  token=$(gcloud auth print-access-token)
  helm registry login -u oauth2accesstoken --password "$token" https://europe-west3-docker.pkg.dev

# Template the Helm chart for a given chart name
template-operator chartName:
  helm template --dry-run --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-staging.yaml {{ chartName }} ./charts/hoprd-operator/

# Template the Helm chart for cluster-hoprd
template-cluster:
  helm template --dry-run --namespace hoprd-operator-sample --create-namespace -f ./charts/cluster-hoprd/values-staging.yaml app-green ./charts/cluster-hoprd/

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
  nohup cargo run &

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