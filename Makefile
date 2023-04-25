gcp-login:
	gcloud auth configure-docker europe-west3-docker.pkg.dev
	gcloud auth application-default print-access-token | helm registry login -u oauth2accesstoken --password-stdin https://europe-west3-docker.pkg.dev

build:
	cargo build

run:
	nohup cargo run &

helm-test:
	helm install --dry-run --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-testing.yaml hoprd-operator ./charts/hoprd-operator/

helm-install:
	helm install --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-testing.yaml hoprd-operator ./charts/hoprd-operator/

helm-uninstall:
	helm uninstall --namespace hoprd-operator hoprd-operator

helm-upgrade:
	helm upgrade --namespace hoprd-operator --create-namespace -f ./charts/hoprd-operator/values-testing.yaml hoprd-operator ./charts/hoprd-operator/
	sleep 3
	kubectl delete deployment -n hoprd-operator hoprd-operator-controller

helm-package-operator:
	helm repo add mittwald https://helm.mittwald.de
	helm repo update
	helm dependency build charts/hoprd-operator
	helm package charts/hoprd-operator --version 0.0.1

helm-package-cluster:
	helm package charts/cluster-hoprd --version 0.0.1

helm-publish-operator:
	helm push hoprd-operator-0.0.1.tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

helm-publish-cluster:
	helm push cluster-hoprd-0.0.1.tgz oci://europe-west3-docker.pkg.dev/hoprassociation/helm-charts

create-node:
	kubectl apply -f hoprd-node-1.yaml

delete-node:
	kubectl delete -f hoprd-node-1.yaml

docker-build:
	docker build -t gcr.io/hoprassociation/hoprd-operator:latest --platform linux/amd64 --progress plain .

docker-push:
	docker push gcr.io/hoprassociation/hoprd-operator:latest

create-cluster:
	kubectl apply -f cluster-hoprd.yaml

delete-cluster:
	kubectl delete -f cluster-hoprd.yaml

UNLOCK_PATCH_DATA="{\"metadata\":{\"labels\":{\"hoprds.hoprnet.org/locked\": \"false\"}}}"
LOCK_PATCH_DATA="{\"metadata\":{\"labels\":{\"hoprds.hoprnet.org/locked\": \"true\"}}}"

lock-secrets:
	for secret in `kubectl get secrets -n hoprd-operator -l hoprds.hoprnet.org/locked=false -o jsonpath="{.items[*].metadata.name}"`; do kubectl patch secret -n hoprd-operator $$secret --type merge --patch $(LOCK_PATCH_DATA);  done

unlock-secrets:
	for secret in `kubectl get secrets -n hoprd-operator -l hoprds.hoprnet.org/locked=true -o jsonpath="{.items[*].metadata.name}"`; do kubectl patch secret -n hoprd-operator $$secret --type merge --patch $(UNLOCK_PATCH_DATA);  done
