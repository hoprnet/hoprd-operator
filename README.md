# Hopr Kubernetes operator

A Kubernetes operator built on top of [kube-rs](https://github.com/clux/kube-rs) project to handle `hoprd` nodes

## Prerequisites

Bear in mind that in order for the wallet to work correctly, the wallet has be compliant with the following requirements:
- Hold a Developer NFT for `monte_rosa` which has been staked for Season 6 or later
- Have enough funds in mHOPR
- Have enough funds in xDAI

## Usage

This operator provides two CRD:
- **Hoprd**: This resource manage a single hoprd node. See the [specifications](./charts/hoprd-operator/templates/crd-hoprd.yaml) for details about what can be configured on an specific hoprd node.
- **ClusterHoprd**: This resource manage a cluster of related hoprd nodes. See the [specifications](./charts/hoprd-operator/templates/crd-cluster-hoprd.yaml) for details about what can be configured on a cluster of nodes.


Note: Keep in mind that the `network` attributes of a node cannot be modified.

## Development

1. Use `kubectl apply -f hoprds.hoprnet.org.yaml` to create the CustomResourceDefinition inside Kubernetes.
2. Build the project with `cargo build`. If the build fails, make sure `libssl-dev` is available.
3. Run the operator using `cargo run`. It will run outside of the Kubernetes cluster and connect to the Kubernetes REST API using the account inside the `KUBECONFIG` automatically.

Finally, a custom `Hoprd` resource can be created with `kubectl apply -f hoprd-node-1.yaml`. A new deployment with `Hoprd` node will be created. 


### CRD

Include a given CRD into the Rust code:
````
kopium servicemonitors.monitoring.coreos.com -A > src/service_monitor.rs
````

### Container
Build the hoprd-operator container using in the repo root:

```shell
docker build -t europe-west3-docker.pkg.dev/hoprassociation/docker-images/hoprd-operator:latest .
```

