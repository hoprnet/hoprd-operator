# Hopr Kubernetes operator

A Kubernetes operator built on top of [kube-rs](https://github.com/clux/kube-rs) project to handle `hoprd` nodes

## Development

1. Use `kubectl apply -f hoprds.hoprnet.org.yaml` to create the CustomResourceDefinition inside Kubernetes.
2. Build the project with `cargo build`. If the build fails, make sure `libssl-dev` is available.
3. Run the operator using `cargo run`. It will run outside of the Kubernetes cluster and connect to the Kubernetes REST API using the account inside the `KUBECONFIG` automatically.

Finally, a custom `Hoprd` resource can be created with `kubectl apply -f hoprd-node-1.yaml`. A new deployment with `Hoprd` node will be created. 


## CRD

Include a given CRD into the Rust code:
````
kopium servicemonitors.monitoring.coreos.com -A > src/service_monitor.rs
````

## Container
Build the hoprd-operator container using in the repo root:

```shell
docker build -t gcr.io/hoprassociation/hopr-operator:latest .
```
