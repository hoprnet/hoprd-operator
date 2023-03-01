
build:
	cargo build

run:
	nohup cargo run &

upgrade:
	helm upgrade --namespace hoprd --create-namespace -f ./charts/hoprd-operator/testValues.yaml hopr-operator ./charts/hoprd-operator/
	sleep 3
	kubectl delete deployment -n hoprd hopr-operator

test: delete-node create-node

delete-node:
	kubectl delete -f hoprd-node-1.yaml

create-node:
	
	kubectl apply -f hoprd-node-1.yaml
