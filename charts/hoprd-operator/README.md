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
| `environmentName`  | Name of the environment                            | `""`  |

### Hopr AdminUI Parameters

| Name                               | Description                                                                                           | Value                                      |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `adminUI.enabled`                  | Whether to install Hopr Admin UI                                                                      | `true`                                     |
| `adminUI.replicas`                 | Replicas for AdminUI deployment                                                                       | `1`                                        |
| `adminUI.commonLabels`             | Labels to add to AdminUI deployment                                                                   | `{}`                                       |
| `adminUI.commonAnnotations`        | Annotations to AdminUI deployment                                                                     | `{}`                                       |
| `adminUI.nodeSelector`             | Object containing node selection constraint to AdminUI deployment                                     | `{}`                                       |
| `adminUI.resources`                | Resource specification to AdminUI deployment                                                          | `{}`                                       |
| `adminUI.tolerations`              | Tolerations specifications to AdminUI deployment                                                      | `[]`                                       |
| `adminUI.affinity`                 | Affinity specifications to AdminUI deployment                                                         | `{}`                                       |
| `adminUI.image.registry`           | Docker registry to AdminUI deployment                                                                 | `europe-west3-docker.pkg.dev`              |
| `adminUI.image.repository`         | Docker image repository to AdminUI deployment                                                         | `hoprassociation/docker-images/hopr-admin` |
| `adminUI.image.tag`                | Docker image tag to AdminUI deployment                                                                | `stable`                                   |
| `adminUI.image.pullPolicy`         | Pull policy to AdminUI deployment as deinfed in                                                       | `Always`                                   |
| `adminUI.ingress.enabled`          | Enable ingress record generation                                                                      | `true`                                     |
| `adminUI.ingress.pathType`         | Ingress path type                                                                                     | `ImplementationSpecific`                   |
| `adminUI.ingress.ingressClassName` | IngressClass that will be be used to implement the Ingress                                            | `""`                                       |
| `adminUI.ingress.hostname`         | Default host for the ingress record                                                                   | `admin.hoprd.cluster.local`                |
| `adminUI.ingress.path`             | Default path for the ingress record                                                                   | `/`                                        |
| `adminUI.ingress.annotations`      | Additional custom annotations for the ingress record                                                  | `{}`                                       |
| `adminUI.ingress.extraPaths`       | An array with additional arbitrary paths that may need to be added to the ingress under the main host | `[]`                                       |

### Hopr Operator Parameters

| Name                                               | Description                                                        | Value                                          |
| -------------------------------------------------- | ------------------------------------------------------------------ | ---------------------------------------------- |
| `operator.replicas`                                | Replicas for operator deployment                                   | `1`                                            |
| `operator.strategy`                                | Strategy for operator deployment                                   | `Recreate`                                     |
| `operator.hopli.registry`                          | Docker registry to hopli image                                     | `europe-west3-docker.pkg.dev`                  |
| `operator.hopli.repository`                        | Docker image to hopli binary                                       | `hoprassociation/docker-images/hopli`          |
| `operator.hopli.tag`                               | Docker image tag to hopli image                                    | `latest`                                       |
| `operator.tokenAmount.hopr`                        | Hopr token amount to fund nodes                                    | `10`                                           |
| `operator.tokenAmount.native`                      | Native(xDAI) token amount to fund nodes                            | `0.01`                                         |
| `operator.fastSync.enabled`                        | Enable Fast Sync                                                   | `false`                                        |
| `operator.fastSync.bucketName`                     | Name of the bucket to store the logs                               | `""`                                           |
| `operator.fastSync.namespaces`                     | Allowed namespaces for uploading logs                              | `[]`                                           |
| `operator.fastSync.crossplane.provider.crossplane` | Crossplane provider name for Crossplane                            | `""`                                           |
| `operator.fastSync.crossplane.provider.upbound`    | Crossplane provider name for GCP                                   | `""`                                           |
| `operator.fastSync.crossplane.gcpProjectId`        | GCP Project ID                                                     | `""`                                           |
| `operator.defaultHoprdEnvVars`                     | Environment variables to be set in the Hoprd Nodes                 | `{}`                                           |
| `operator.commonLabels`                            | Labels to add to all operator related objects                      | `{}`                                           |
| `operator.commonAnnotations`                       | Annotations to to all operator related objects                     | `{}`                                           |
| `operator.extraEnvVars`                            | Array of extra environment variables                               | `[]`                                           |
| `operator.ingress.ingressClassName`                | Name of the ingress class name to be used by Hoprd nodes           | `""`                                           |
| `operator.ingress.dnsDomain`                       | Name of the DNS suffix domain to be added to Hoprd nodes           | `""`                                           |
| `operator.ingress.namespace`                       | Namespace of the running ingress controller                        | `""`                                           |
| `operator.ingress.annotations`                     | Annotations to be added to ingress resources of Hoprd nodes        | `{}`                                           |
| `operator.ingress.loadBalancerIP`                  | Public IP of the LoadBalancer Service for the Ingress              | `""`                                           |
| `operator.ingress.ports.min`                       | Starting port to open on Ingress controller                        | `9000`                                         |
| `operator.ingress.ports.max`                       | End port to open on Ingress controller                             | `10000`                                        |
| `operator.ingress.deploymentName`                  | Labels selector to choose the Nginx deployment and service         | `""`                                           |
| `operator.nodeSelector`                            | Object containing node selection constraint to operator deployment | `{}`                                           |
| `operator.resources`                               | Resource specification to operator deployment                      | `{}`                                           |
| `operator.tolerations`                             | Tolerations specifications to operator deployment                  | `[]`                                           |
| `operator.affinity`                                | Affinity specifications to operator deployment                     | `{}`                                           |
| `operator.image.registry`                          | Docker registry to operator deployment                             | `europe-west3-docker.pkg.dev`                  |
| `operator.image.repository`                        | Docker image repository to operator deployment                     | `hoprassociation/docker-images/hoprd-operator` |
| `operator.image.tag`                               | Docker image tag to operator deployment                            | `""`                                           |
| `operator.image.pullPolicy`                        | Pull policy to operator deployment as deinfed in                   | `IfNotPresent`                                 |
| `operator.persistence.size`                        | Size of the persistence Volume                                     | `500Mi`                                        |
| `operator.persistence.storageClassName`            | Name of the storage class                                          | `""`                                           |

### Service Parameters

| Name                               | Description                                                      | Value       |
| ---------------------------------- | ---------------------------------------------------------------- | ----------- |
| `service.type`                     | service type                                                     | `ClusterIP` |
| `service.ports.http`               | service HTTP port number                                         | `8080`      |
| `service.ports.name`               | service HTTP port name                                           | `http`      |
| `service.nodePorts.http`           | Node port for HTTP                                               | `""`        |
| `service.clusterIP`                | service Cluster IP                                               | `""`        |
| `service.loadBalancerIP`           | service Load Balancer IP                                         | `""`        |
| `service.loadBalancerSourceRanges` | service Load Balancer sources                                    | `[]`        |
| `service.externalTrafficPolicy`    | service external traffic policy                                  | `Cluster`   |
| `service.sessionAffinity`          | Control where client requests go, to the same pod or round-robin | `None`      |
