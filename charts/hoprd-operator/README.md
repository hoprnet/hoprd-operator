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

### Hopr AdminUI Parameters

| Name                                             | Description                                                                                           | Value                                      |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `hoprdOperator.adminUI.enabled`                  | Whether to install Hopr Admin UI                                                                      | `true`                                     |
| `hoprdOperator.adminUI.replicas`                 | Replicas for AdminUI deployment                                                                       | `1`                                        |
| `hoprdOperator.adminUI.resources`                | Resource specification to AdminUI deployment                                                          | `{}`                                       |
| `hoprdOperator.adminUI.image.registry`           | Docker registry to AdminUI deployment                                                                 | `europe-west3-docker.pkg.dev`              |
| `hoprdOperator.adminUI.image.repository`         | Docker image repository to AdminUI deployment                                                         | `hoprassociation/docker-images/hopr-admin` |
| `hoprdOperator.adminUI.image.tag`                | Docker image tag to AdminUI deployment                                                                | `stable`                                   |
| `hoprdOperator.adminUI.image.pullPolicy`         | Pull policy to AdminUI deployment as deinfed in                                                       | `Always`                                   |
| `hoprdOperator.adminUI.service.type`             | service type                                                                                          | `ClusterIP`                                |
| `hoprdOperator.adminUI.service.ports.http`       | service HTTP port number                                                                              | `8080`                                     |
| `hoprdOperator.adminUI.service.ports.name`       | service HTTP port name                                                                                | `http`                                     |
| `hoprdOperator.adminUI.ingress.enabled`          | Enable ingress record generation                                                                      | `true`                                     |
| `hoprdOperator.adminUI.ingress.pathType`         | Ingress path type                                                                                     | `ImplementationSpecific`                   |
| `hoprdOperator.adminUI.ingress.ingressClassName` | IngressClass that will be be used to implement the Ingress                                            | `nginx`                                    |
| `hoprdOperator.adminUI.ingress.hostname`         | Default host for the ingress record                                                                   | `admin.hoprd.cluster.local`                |
| `hoprdOperator.adminUI.ingress.path`             | Default path for the ingress record                                                                   | `/`                                        |
| `hoprdOperator.adminUI.ingress.annotations`      | Additional custom annotations for the ingress record                                                  | `{}`                                       |
| `hoprdOperator.adminUI.ingress.extraPaths`       | An array with additional arbitrary paths that may need to be added to the ingress under the main host | `[]`                                       |

### Hopr Operator Parameters

| Name                                                    | Description                                                 | Value                                          |
| ------------------------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------- |
| `hoprdOperator.hopli.registry`                          | Docker registry to hopli image                              | `europe-west3-docker.pkg.dev`                  |
| `hoprdOperator.hopli.repository`                        | Docker image to hopli binary                                | `hoprassociation/docker-images/hopli`          |
| `hoprdOperator.hopli.tag`                               | Docker image tag to hopli image                             | `latest`                                       |
| `hoprdOperator.fastSync.enabled`                        | Enable Fast Sync                                            | `false`                                        |
| `hoprdOperator.fastSync.bucketName`                     | Name of the bucket to store the logs                        | `""`                                           |
| `hoprdOperator.fastSync.namespaces`                     | Allowed namespaces for uploading logs                       | `[]`                                           |
| `hoprdOperator.fastSync.crossplane.provider.crossplane` | Crossplane provider name for Crossplane                     | `""`                                           |
| `hoprdOperator.fastSync.crossplane.provider.upbound`    | Crossplane provider name for GCP                            | `""`                                           |
| `hoprdOperator.fastSync.crossplane.gcpProjectId`        | GCP Project ID                                              | `""`                                           |
| `hoprdOperator.defaultHoprdEnvVars`                     | Environment variables to be set in the Hoprd Nodes          | `{}`                                           |
| `hoprdOperator.resources`                               | Resource specification to operator deployment               | `{}`                                           |
| `hoprdOperator.extraEnvVars`                            | Array of extra environment variables                        | `[]`                                           |
| `hoprdOperator.image.registry`                          | Docker registry to operator deployment                      | `europe-west3-docker.pkg.dev`                  |
| `hoprdOperator.image.repository`                        | Docker image repository to operator deployment              | `hoprassociation/docker-images/hoprd-operator` |
| `hoprdOperator.image.tag`                               | Docker image tag to operator deployment                     | `""`                                           |
| `hoprdOperator.image.pullPolicy`                        | Pull policy to operator deployment as deinfed in            | `IfNotPresent`                                 |
| `hoprdOperator.ingress.ingressClassName`                | Name of the ingress class name to be used by Hoprd nodes    | `nginx`                                        |
| `hoprdOperator.ingress.dnsDomain`                       | Name of the DNS suffix domain to be added to Hoprd nodes    | `""`                                           |
| `hoprdOperator.ingress.namespace`                       | Namespace of the running ingress controller                 | `""`                                           |
| `hoprdOperator.ingress.annotations`                     | Annotations to be added to ingress resources of Hoprd nodes | `{}`                                           |
| `hoprdOperator.ingress.loadBalancerIP`                  | Public IP of the LoadBalancer Service for the Ingress       | `""`                                           |
| `hoprdOperator.ingress.ports.min`                       | Starting port to open on Ingress controller                 | `9000`                                         |
| `hoprdOperator.ingress.ports.max`                       | End port to open on Ingress controller                      | `10000`                                        |
| `hoprdOperator.ingress.deploymentName`                  | Labels selector to choose the Nginx deployment and service  | `""`                                           |
| `hoprdOperator.persistence.size`                        | Size of the persistence Volume                              | `500Mi`                                        |
| `hoprdOperator.persistence.storageClassName`            | Name of the storage class                                   | `""`                                           |
