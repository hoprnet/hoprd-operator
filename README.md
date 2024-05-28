# Hopr Kubernetes operator

A Kubernetes operator built on top of [kube-rs](https://github.com/clux/kube-rs) project to handle `hoprd` nodes

## Prerequisites

Keep in mind that in order for the wallet to work correctly, the wallet has be compliant with the following requirements:
- Hold a staked developer NFT for the specified network (`rotsee`, `dufour`) 
- Have enough funds in wxHOPR
- Have enough funds in xDAI

## Usage

This operator provides the following CRD:
- **IdentityPool**: This resource manages a pool of identities issued from the same wallet. This resources requires that a previous `Secret` is created holding the private key of the wallet. On the creation of a `IdentityPool` resource, the operator will create the corresponding service account, service monitor and cronjob for auto-funding. 
Bellow is described the example of a secret holding the required information by the IdentityPool
````
apiVersion: v1
data:
  DEPLOYER_PRIVATE_KEY: ??????
  HOPRD_API_TOKEN: ??????
  IDENTITY_PASSWORD: ???????
kind: Secret
metadata:
  name: hoprd-core-rotsee-wallet
  namespace: core-team
type: Opaque
````
Bellow is described the contents of a sample IdentityPool which has 7 child IdentityHoprd defined, but only 5 of them are being used.
````
apiVersion: hoprnet.org/v1alpha2
kind: IdentityPool
metadata:
  finalizers:
  - hoprds.hoprnet.org/finalizer
  name: hoprd-core-rotsee
  namespace: core-team
spec:
  network: rotsee
  secretName: hoprd-core-rotsee-wallet
status:
  locked: 5
  phase: Ready
  size: 7
````
- **IdentityHoprd**: This resource manages the identity of a Hoprd resource. A `IdentityHoprd` always belongs to a `IdentityPool`. This resource stores all the information that is needed to be able to run a Hoprd node. It contains the base64 encoded format of the identity file, the peerId, node address, safe address and module address. The only missing part to run the node is the identity password which is stored in the `Secret` referenced by the `IdentityPool` to which this resource belongs. On the creation of a `IdentityHoprd` resource, the operator will create the persistent volume claim. This means that the storage of the node is not linked to the `Hoprd` resource but to the `IdentityHoprd` resources, being able to remove the `Hoprd` resources or scale it down without loosing the database of the `IdentityHoprd`.
Bellow is described the contents of a sample IdentityHoprd which is in used by the HoprdNode named `hoprd-core-rotsee-1`.
````
apiVersion: hoprnet.org/v1alpha2
kind: IdentityHoprd
metadata:
  finalizers:
  - hoprds.hoprnet.org/finalizer
  name: hoprd-core-rotsee-1
  namespace: core-team
  ownerReferences:
  - apiVersion: hoprnet.org/v1alpha2
    controller: true
    kind: IdentityPool
    name: hoprd-core-rotsee
spec:
  identityFile: <ENCODED BAS64 identityFile>
  identityPoolName: hoprd-core-rotsee
  moduleAddress: 0x2031b4494500E7b112342741EFA5f61b2d8E6331
  nativeAddress: 0xa34e407d9ddbe25207f250c4193b42fef68ba903
  peerId: 12D3KooWLDer461CFi1fB5pwQKGT72PAvGFMAxH9ALNwNjG7A5ep
  safeAddress: 0x0CDecAFf277C296665f31aAC0957a3A3151B6159
status:
  hoprdNodeName: hoprd-core-rotsee-1
  phase: InUse
````
- **ClusterHoprd**: This resource manages a cluster of related hoprd nodes. See the [specifications](./charts/hoprd-operator/templates/crd-cluster-hoprd.yaml) for details about what can be configured on a cluster of nodes. On the creation of a `ClusterHoprd` resource, the operator will create a bunch of Hoprd resources with different names and using different `IdentityHoprd` from the same `IdentityPool`. Bellow is described the contents of a sample ClusterHoprd which defines 5 nodes and uses the `IdentityPool` _hoprd-core-rotsee_.
````
apiVersion: hoprnet.org/v1alpha2
kind: ClusterHoprd
metadata:
  finalizers:
  - hoprds.hoprnet.org/finalizer
  name: hoprd-core-rotsee
  namespace: core-team
spec:
  config: |
    hopr:
      chain:
        network: rotsee
        provider: http://gnosis-rpc-provider.rpc-provider.svc.cluster.local:8545
      strategy:
        on_fail_continue: true
        allow_recursive: false
        strategies: []
  deployment:
    resources: |
      limits:
        cpu: 2000m
        memory: 1Gi
      requests:
        cpu: 2000m
        memory: 1Gi
  enabled: true
  forceIdentityName: true
  identityPoolName: hoprd-core-rotsee
  replicas: 5
  supportedRelease: saint-louis
  version: 2.1.0
status:
  currentNodes: 5
  phase: Ready
````
- **Hoprd**: This resource manages a single hoprd node. See the [specifications](./charts/hoprd-operator/templates/crd-hoprd.yaml) for details about what can be configured on an specific hoprd node. On the creation of a `Hoprd` resource, the operator will create the corresponding deployment, service and ingress if required.
Bellow is described the contents of a sample `Hoprd` which belongs to the previously created cluster.
````
apiVersion: hoprnet.org/v1alpha2
kind: Hoprd
metadata:
  finalizers:
  - hoprds.hoprnet.org/finalizer
  name: hoprd-core-rotsee-1
  namespace: core-team
  ownerReferences:
  - apiVersion: hoprnet.org/v1alpha2
    controller: true
    kind: ClusterHoprd
    name: hoprd-core-rotsee
spec:
  config: |
    hopr:
      chain:
        network: rotsee
        provider: http://gnosis-rpc-provider.rpc-provider.svc.cluster.local:8545
      strategy:
        on_fail_continue: true
        allow_recursive: false
        strategies: []
  deleteDatabase: false
  deployment:
    resources: |
      limits:
        cpu: 2000m
        memory: 1Gi
      requests:
        cpu: 2000m
        memory: 1Gi
  enabled: true
  identityName: hoprd-core-rotsee-1
  identityPoolName: hoprd-core-rotsee
  supportedRelease: saint-louis
  version: 2.1.0
status:
  identityName: hoprd-core-rotsee-1
  phase: Running
````

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

