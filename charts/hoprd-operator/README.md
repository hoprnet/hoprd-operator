<!--- app-name: Hopr Operator -->

# Hopr Operator Chart

This chart packages all the kubernetes resources needed to install Hopr opreator on Kubernetes

## Installing

```console
$ helm repo add hoprd-operator git+https://github.com/hoprnet/hoprd-operator@charts?ref=master
$ helm install hoprd-operator hoprd-operator/hoprd-operator
```

## Uninstalling the Chart

To uninstall/delete the release:

```console
helm delete hoprd-operator
```

The command removes all the Kubernetes components associated with the chart and deletes the release.

## Creating a pull request

Chart version `Chart.yaml` should be increased according to [semver](http://semver.org/)

## Parameters

### Common parameters

| Name               | Description                                        | Value |
| ------------------ | -------------------------------------------------- | ----- |
| `nameOverride`     | String to partially override common.names.fullname | `""`  |
| `fullnameOverride` | String to fully override common.names.fullname     | `""`  |

### Replicator Parameters

| Name                 | Description                                                                                                     | Value  |
| -------------------- | --------------------------------------------------------------------------------------------------------------- | ------ |
| `replicator.enabled` | Install the Helm Chart dependency Reflector. See more info at https://github.com/mittwald/kubernetes-replicator | `true` |

### Hopr AdminUI Parameters

| Name                        | Description                                                       | Value                        |
| --------------------------- | ----------------------------------------------------------------- | ---------------------------- |
| `adminUI.enabled`           | Whether to install Hopr Admin UI                                  | `true`                       |
| `adminUI.replicas`          | Replicas for AdminUI deployment                                   | `1`                          |
| `adminUI.commonLabels`      | Labels to add to AdminUI deployment                               | `{}`                         |
| `adminUI.commonAnnotations` | Annotations to AdminUI deployment                                 | `{}`                         |
| `adminUI.nodeSelector`      | Object containing node selection constraint to AdminUI deployment | `{}`                         |
| `adminUI.resources`         | Resource specification to AdminUI deployment                      | `{}`                         |
| `adminUI.tolerations`       | Tolerations specifications to AdminUI deployment                  | `[]`                         |
| `adminUI.affinity`          | Affinity specifications to AdminUI deployment                     | `{}`                         |
| `adminUI.image.registry`    | Docker registry to AdminUI deployment                             | `gcr.io`                     |
| `adminUI.image.repository`  | Docker image repository to AdminUI deployment                     | `hoprassociation/hopr-admin` |
| `adminUI.image.tag`         | Docker image tag to AdminUI deployment                            | `riga`                       |
| `adminUI.image.pullPolicy`  | Pull policy to AdminUI deployment as deinfed in                   | `IfNotPresent`               |

### Hopr Operator Parameters

| Name                                    | Description                                                                                                      | Value                           |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `operator.replicas`                     | Replicas for operator deployment                                                                                 | `1`                             |
| `operator.privateKey`                   | Private Key of the Wallet used to make blockchain transactions like: register in network registry or fund nodes. | `""`                            |
| `operator.secretName`                   | Name of the secret custoding the private Key of the Wallet used to make blockchain transactions                  | `""`                            |
| `operator.secretKeyName`                | Key name within the Secret                                                                                       | `PRIVATE_KEY`                   |
| `operator.commonLabels`                 | Labels to add to all operator related objects                                                                    | `{}`                            |
| `operator.commonAnnotations`            | Annotations to to all operator related objects                                                                   | `{}`                            |
| `operator.persistence.size`             | Size of the persistence Volume                                                                                   | `50Mi`                          |
| `operator.persistence.storageClassName` | Name of the storage class                                                                                        | `ceph-filesystem`               |
| `operator.nodeSelector`                 | Object containing node selection constraint to operator deployment                                               | `{}`                            |
| `operator.resources`                    | Resource specification to operator deployment                                                                    | `{}`                            |
| `operator.tolerations`                  | Tolerations specifications to operator deployment                                                                | `[]`                            |
| `operator.affinity`                     | Affinity specifications to operator deployment                                                                   | `{}`                            |
| `operator.image.registry`               | Docker registry to operator deployment                                                                           | `gcr.io`                        |
| `operator.image.repository`             | Docker image repository to operator deployment                                                                   | `hoprassociation/hoprd-operator` |
| `operator.image.tag`                    | Docker image tag to operator deployment                                                                          | `0.1.4`                         |
| `operator.image.pullPolicy`             | Pull policy to operator deployment as deinfed in                                                                 | `IfNotPresent`                  |

### Service Parameters

| Name                               | Description                                                      | Value       |
| ---------------------------------- | ---------------------------------------------------------------- | ----------- |
| `service.type`                     | service type                                                     | `ClusterIP` |
| `service.ports.http`               | service HTTP port number                                         | `3000`      |
| `service.ports.name`               | service HTTP port name                                           | `http`      |
| `service.nodePorts.http`           | Node port for HTTP                                               | `""`        |
| `service.clusterIP`                | service Cluster IP                                               | `""`        |
| `service.loadBalancerIP`           | service Load Balancer IP                                         | `""`        |
| `service.loadBalancerSourceRanges` | service Load Balancer sources                                    | `[]`        |
| `service.externalTrafficPolicy`    | service external traffic policy                                  | `Cluster`   |
| `service.sessionAffinity`          | Control where client requests go, to the same pod or round-robin | `None`      |

### Service Parameters

| Name                       | Description                                                                                           | Value                       |
| -------------------------- | ----------------------------------------------------------------------------------------------------- | --------------------------- |
| `ingress.enabled`          | Enable ingress record generation                                                                      | `false`                     |
| `ingress.pathType`         | Ingress path type                                                                                     | `ImplementationSpecific`    |
| `ingress.ingressClassName` | IngressClass that will be be used to implement the Ingress                                            | `nginx`                     |
| `ingress.hostname`         | Default host for the ingress record                                                                   | `admin.hoprd.cluster.local` |
| `ingress.path`             | Default path for the ingress record                                                                   | `/`                         |
| `ingress.annotations`      | Additional custom annotations for the ingress record                                                  | `{}`                        |
| `ingress.extraPaths`       | An array with additional arbitrary paths that may need to be added to the ingress under the main host | `[]`                        |
