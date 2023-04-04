
build:
	cargo build

run:
	nohup cargo run &

install:
	helm install --namespace hoprd --create-namespace -f ./charts/hoprd-operator/values-testing.yaml hoprd-operator ./charts/hoprd-operator/

uninstall:
	helm uninstall --namespace hoprd hoprd-operator

upgrade:
	helm upgrade --namespace hoprd --create-namespace -f ./charts/hoprd-operator/values-testing.yaml hoprd-operator ./charts/hoprd-operator/
	sleep 3
	kubectl delete deployment -n hoprd hoprd-operator-controller

test: delete-node create-node

delete-node:
	kubectl delete -f hoprd-node-1.yaml

create-node:
	kubectl apply -f hoprd-node-1.yaml

docker-build:
	docker build -t gcr.io/hoprassociation/hoprd-operator:latest --platform linux/amd64 --progress plain .

docker-push:
	docker push gcr.io/hoprassociation/hoprd-operator:latest
