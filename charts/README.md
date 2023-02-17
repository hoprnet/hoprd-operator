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

| Name                 | Description                                                                                                      | Value                           |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `privateKey`         | Private Key of the Wallet used to make blockchain transactions like: register in network registry or fund nodes. | `""`                            |
| `secretName`         | Name of the secret custoding the private Key of the Wallet used to make blockchain transactions                  | `""`                            |
| `secretKeyName`      | Key name within the Secret                                                                                       | `PRIVATE_KEY`                   |
| `nodeSelector`       | Object containing node selection constraint.                                                                     | `{}`                            |
| `resources`          | Resource specification                                                                                           | `{}`                            |
| `tolerations`        | Tolerations specifications                                                                                       | `[]`                            |
| `affinity`           | Affinity specifications                                                                                          | `{}`                            |
| `image.registry`     | Docker registry                                                                                                  | `gcr.io`                        |
| `image.repository`   | Docker image repository                                                                                          | `hoprassociation/hopr-operator` |
| `image.tag`          | Docker image tag                                                                                                 | `latest`                        |
| `image.pullPolicy`   | Pull policy as deinfed in                                                                                        | `IfNotPresent`                  |
| `service.ports.name` | Name of the API service port                                                                                     | `api`                           |

### Metrics parameters

| Name                                       | Description                                                                      | Value   |
| ------------------------------------------ | -------------------------------------------------------------------------------- | ------- |
| `metrics.serviceMonitor.enabled`           | Specify if a ServiceMonitor will be deployed for Hopr Operator                   | `false` |
| `metrics.serviceMonitor.namespace`         | Namespace in which deploy the service Monitor                                    | `""`    |
| `metrics.serviceMonitor.namespaceSelector` | Namespaces which will be scrapped for metrics                                    | `[]`    |
| `metrics.serviceMonitor.jobLabel`          | The name of the label on the target service to use as the job name in Prometheus | `hoprd` |
| `metrics.serviceMonitor.honorLabels`       | honorLabels chooses the metric's labels on collisions with target labels         | `false` |
| `metrics.serviceMonitor.interval`          | Interval at which metrics should be scraped.                                     | `""`    |
| `metrics.serviceMonitor.scrapeTimeout`     | Timeout after which the scrape is ended                                          | `""`    |
| `metrics.serviceMonitor.metricRelabelings` | Specify additional relabeling of metrics                                         | `[]`    |
| `metrics.serviceMonitor.relabelings`       | Specify general relabeling                                                       | `[]`    |
| `metrics.serviceMonitor.selector`          | Hord node instance selector labels                                               | `{}`    |
