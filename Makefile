
.PHONY: help
help: ## Show help of available commands
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' Makefile | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

gcp-login: ## Login into GCP
	gcloud auth configure-docker europe-west3-docker.pkg.dev
	gcloud auth application-default print-access-token | helm registry login -u oauth2accesstoken --password-stdin https://europe-west3-docker.pkg.dev

.PHONY: lint-rust
lint-rust: ## run linter for Rust
	cargo fmt --check
	cargo clippy -- -Dwarnings

build: ## Rust build
	cargo build

run: ## Rust run
	nohup cargo run &

helm-template: ## Print helm resources
	helm template --dry-run --namespace hoprd-operator --create-namespace -f ./charts/$(chart)/values-stage.yaml $(chart) ./charts/$(chart)/

helm-test: ## Install dry helm resources
	helm install --dry-run --namespace hoprd-operator --create-namespace -f ./charts/$(chart)/values-stage.yaml $(chart) ./charts/$(chart)/

helm-lint: ## Lint Helm
	helm lint -f ./charts/$(chart)/values-stage.yaml ./charts/$(chart)

helm-install: ## Install helm chart using values-stage.yaml file
	helm install --namespace hoprd-operator --create-namespace -f ./charts/$(chart)/values-stage.yaml $(chart) ./charts/$(chart)/

helm-uninstall: ## Uninstall helm chart
	helm uninstall --namespace hoprd-operator $(chart)

helm-upgrade: ## Update helm-chart templates into cluster and remove deployment to be run within VsCode in debug mode
	helm upgrade --namespace hoprd-operator --create-namespace -f ./charts/$(chart)/values-stage.yaml $(chart) ./charts/$(chart)/
	# sleep 3
	# kubectl delete deployment -n hoprd-operator hoprd-operator-controller

helm-package: ## Creates helm package
	helm package charts/hoprd-operator --version $$(yq '.version' charts/hoprd-operator/Chart.yaml)
	helm package charts/cluster-hoprd --version $$(yq '.version' charts/cluster-hoprd/Chart.yaml)

helm-publish: ## Deploys helm package to GCP artifact registry
	helm push hoprd-operator-$$(yq '.version' charts/hoprd-operator/Chart.yaml).tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts
	helm push cluster-hoprd-$$(yq '.version' charts/cluster-hoprd/Chart.yaml).tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

docker-build: ## Builds docker image
	docker build -t europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest --platform linux/amd64 --progress plain .

docker-push: ## Deploys docker image into GCP Artifact registry
	docker push europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest

create-identity: ## Create identity resources
	kubectl apply -f ../hoprd-nodes/rotsee/devops/identities/sealed-secret-hoprd-operator.yaml
	kubectl apply -f ../hoprd-nodes/rotsee/devops/identities/identity-pool.yaml
	kubectl apply -f ../hoprd-nodes/rotsee/devops/identities/identity-hoprds-devops-1.yaml
	kubectl apply -f ../hoprd-nodes/rotsee/devops/identities/identity-hoprds-devops-2.yaml
	# kubectl patch -n hoprd-operator IdentityPool pool-hoprd-operator --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":1}]'
	# kubectl patch -n rotsee IdentityPool core-rotsee --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":1}]'

delete-identity: ## Deletes identity resources
	# kubectl patch -n hoprd-operator IdentityPool pool-hoprd-operator --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":0}]'
	# kubectl patch -n rotsee IdentityPool core-rotsee --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":0}]'
	kubectl delete -f ./hoprd-nodes/rotsee/devops/identities/identity-hoprds-devops-1.yaml
	kubectl delete -f ./hoprd-nodes/rotsee/devops/identities/identity-hoprds-devops-2.yaml
	kubectl delete -f ../hoprd-nodes/rotsee/devops/identities/identity-pool.yaml
	kubectl delete -f ../hoprd-nodes/rotsee/devops/identities/sealed-secret-hoprd-operator.yaml

