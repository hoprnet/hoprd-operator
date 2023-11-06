
.PHONY: help
help: ## Show help of available commands
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' Makefile | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

gcp-login: ## Login into GCP
	gcloud auth configure-docker europe-west3-docker.pkg.dev
	gcloud auth application-default print-access-token | helm registry login -u oauth2accesstoken --password-stdin https://europe-west3-docker.pkg.dev

build: ## Rust build
	cargo build

run: ## Rust run
	nohup cargo run &

helm-test: ## Print helm resources
	helm install --dry-run --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-stage.yaml hoprd-operator ./charts/hoprd-operator/

helm-install: ## Install helm chart using values-stage.yaml file
	helm install --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-stage.yaml hoprd-operator ./charts/hoprd-operator/

helm-uninstall: ## Uninstall helm chart
	helm uninstall --namespace hoprd-operator hoprd-operator

helm-upgrade: ## Update helm-chart templates into cluster and remove deployment to be run within VsCode in debug mode
	helm upgrade --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-stage.yaml hoprd-operator ./charts/hoprd-operator/
	sleep 3
	kubectl delete deployment -n hoprd-operator hoprd-operator-controller

helm-package-operator: ## Creates helm package for operator Hoprd
	helm package charts/hoprd-operator --version 0.0.1

helm-package-cluster: ## Creates helm package for operator ClusterHoprd
	helm package charts/cluster-hoprd --version 0.0.1

helm-publish-operator: ## Deploys helm package to GCP artifact registry
	helm push hoprd-operator-0.0.1.tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

helm-publish-cluster: ## Deploys helm package to GCP artifact registry
	helm push cluster-hoprd-0.0.1.tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

docker-build: ## Builds docker image
	docker build -t europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest --platform linux/amd64 --progress plain .

docker-push: ## Deploys docker image into GCP Artifact registry
	docker push europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest

create-identity: ## Create identity resources
	kubectl apply -f ./test-data/identity-pool.yaml
	kubectl apply -f ./test-data/identity-hoprd.yaml
	kubectl patch -n rotsee IdentityPool core-rotsee --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":1}]'

delete-identity: ## Deletes identity resources
	kubectl patch -n rotsee IdentityPool core-rotsee --type='json' -p='[{"op": "replace", "path": "/spec/minReadyIdentities", "value":0}]'
	kubectl delete -f ./test-data/identity-hoprd.yaml
	kubectl apply -f ./test-data/identity-pool.yaml

create-node: ## Create hoprd node
	kubectl apply -f ./test-data/hoprd-node.yaml

delete-node: ## Delete hoprd node
	kubectl delete -f ./test-data/hoprd-node.yaml

create-cluster: ## Create cluster hoprd node
	kubectl apply -f ./test-data/cluster-hoprd.yaml

delete-cluster: ## Delete cluster hoprd node
	kubectl delete -f ./test-data/cluster-hoprd.yaml
