<!--- app-name: Hopr Operator -->

# Hopr Operator Chart

This chart packages all the kubernetes resources needed to install Hopr opreator on Kubernetes

## Installing

```console
$ helm repo add hopr-operator git+https://github.com/hoprnet/hopr-operator@charts?ref=master
$ helm install hopr-operator hopr-operator/hopr-operator
```

## Uninstalling the Chart

To uninstall/delete the release:

```console
helm delete hopr-operator
```

The command removes all the Kubernetes components associated with the chart and deletes the release.

## Creating a pull request

Chart version `Chart.yaml` should be increased according to [semver](http://semver.org/)

## Parameters

### Common parameters

| Name                | Description                                        | Value |
| ------------------- | -------------------------------------------------- | ----- |
| `nameOverride`      | String to partially override common.names.fullname | `""`  |
| `fullnameOverride`  | String to fully override common.names.fullname     | `""`  |
| `commonLabels`      | Labels to add to all deployed objects              | `{}`  |
| `commonAnnotations` | Annotations to add to all deployed objects         | `{}`  |

### Hopr Operator Parameters

| Name                           | Description                                                                                                      | Value                           |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `privateKey`                   | Private Key of the Wallet used to make blockchain transactions like: register in network registry or fund nodes. | `""`                            |
| `secretName`                   | Name of the secret custoding the private Key of the Wallet used to make blockchain transactions                  | `""`                            |
| `secretKeyName`                | Key name within the Secret                                                                                       | `PRIVATE_KEY`                   |
| `replicator.enabled`           | Install the Helm Chart dependency Reflector. See more info at https://github.com/mittwald/kubernetes-replicator  | `true`                          |
| `persistence.size`             | Size of the persistence Volume                                                                                   | `50Mi`                          |
| `persistence.storageClassName` | Name of the storage class                                                                                        | `ceph-filesystem`               |
| `nodeSelector`                 | Object containing node selection constraint.                                                                     | `{}`                            |
| `resources`                    | Resource specification                                                                                           | `{}`                            |
| `tolerations`                  | Tolerations specifications                                                                                       | `[]`                            |
| `affinity`                     | Affinity specifications                                                                                          | `{}`                            |
| `image.registry`               | Docker registry                                                                                                  | `gcr.io`                        |
| `image.repository`             | Docker image repository                                                                                          | `hoprassociation/hopr-operator` |
| `image.tag`                    | Docker image tag                                                                                                 | `0.1.4`                         |
| `image.pullPolicy`             | Pull policy as deinfed in                                                                                        | `IfNotPresent`                  |
| `service.ports.name`           | Name of the API service port                                                                                     | `api`                           |
