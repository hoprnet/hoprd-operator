
crd:
	kubectl apply -f hoprds.hoprnet.org.yaml

run:
	nohup cargo run &

create-node:
	kubectl apply -f hoprd-node-1.yaml

delete-node:
	kubectl delete -f hoprd-node-1.yaml