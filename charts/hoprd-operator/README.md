<!--- app-name: Hopr Operator -->

# Hopr Operator Chart

This helm-chart packages all the kubernetes resources needed to install Hopr opreator on Kubernetes

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

### Hopr Operator Parameters

| Name                                                                       | Description                                                                                                                                                                  | Value                                           |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| `hoprdOperator.hopli.registry`                                             | Docker registry to hopli image                                                                                                                                               | `europe-west3-docker.pkg.dev`                   |
| `hoprdOperator.hopli.repository`                                           | Docker image to hopli binary                                                                                                                                                 | `hoprassociation/docker-images/hopli`           |
| `hoprdOperator.hopli.tag`                                                  | Docker image tag to hopli image                                                                                                                                              | `0.20.1`                                        |
| `hoprdOperator.hopli.rpcProviderUrl`                                       | RPC provider URL to be used by hopli binary                                                                                                                                  | `""`                                            |
| `hoprdOperator.kubectl.registry`                                           | Docker registry to the kubectl image                                                                                                                                         | `docker.io`                                     |
| `hoprdOperator.kubectl.repository`                                         | Docker image with kubectl and bash, used by the jobs that talk to the Kubernetes API                                                                                         | `alpine/k8s`                                    |
| `hoprdOperator.kubectl.tag`                                                | Docker image tag of the kubectl image                                                                                                                                        | `1.36.5`                                        |
| `hoprdOperator.fastSync.enabled`                                           | Enable Fast Sync                                                                                                                                                             | `false`                                         |
| `hoprdOperator.fastSync.bucketName`                                        | Name of the bucket to store the logs                                                                                                                                         | `""`                                            |
| `hoprdOperator.fastSync.namespaces`                                        | Allowed namespaces for uploading logs                                                                                                                                        | `[]`                                            |
| `hoprdOperator.fastSync.crossplane.provider.crossplane`                    | Crossplane provider name for Crossplane                                                                                                                                      | `""`                                            |
| `hoprdOperator.fastSync.crossplane.provider.upbound`                       | Crossplane provider name for GCP                                                                                                                                             | `""`                                            |
| `hoprdOperator.fastSync.crossplane.gcpProjectId`                           | GCP Project ID                                                                                                                                                               | `""`                                            |
| `hoprdOperator.defaultHoprdEnvVars`                                        | Environment variables to be set in the Hoprd Nodes                                                                                                                           | `{}`                                            |
| `hoprdOperator.resources`                                                  | Resource specification to operator deployment                                                                                                                                | `{}`                                            |
| `hoprdOperator.extraEnvVars`                                               | Array of extra environment variables                                                                                                                                         | `[]`                                            |
| `hoprdOperator.extraLabels`                                                | Extra labels added to the controller Deployment's own metadata                                                                                                               | `{}`                                            |
| `hoprdOperator.image.registry`                                             | Docker registry to operator deployment                                                                                                                                       | `europe-west3-docker.pkg.dev`                   |
| `hoprdOperator.image.repository`                                           | Docker image repository to operator deployment                                                                                                                               | `hoprassociation/docker-images/hoprd-operator`  |
| `hoprdOperator.image.tag`                                                  | Docker image tag to operator deployment                                                                                                                                      | `""`                                            |
| `hoprdOperator.image.pullPolicy`                                           | Pull policy to operator deployment as defined in                                                                                                                             | `Always`                                        |
| `hoprdOperator.gateway.dnsDomain`                                          | Name of the DNS suffix domain to be added to Hoprd nodes                                                                                                                     | `""`                                            |
| `hoprdOperator.gateway.loadBalancerIP`                                     | Public IP of the LoadBalancer Service of the Gateway (Envoy proxy). It is also the host IP advertised by the Hoprd nodes                                                     | `""`                                            |
| `hoprdOperator.gateway.ports.min`                                          | Starting port to open on the Gateway                                                                                                                                         | `9000`                                          |
| `hoprdOperator.gateway.ports.max`                                          | End port to open on the Gateway                                                                                                                                              | `10000`                                         |
| `hoprdOperator.gateway.certificateIssuer.name`                             | Name of the cert-manager issuer used to create the TLS certificate of each Hoprd node                                                                                        | `""`                                            |
| `hoprdOperator.gateway.certificateIssuer.kind`                             | Kind of the cert-manager issuer (ClusterIssuer or Issuer)                                                                                                                    | `ClusterIssuer`                                 |
| `hoprdOperator.gateway.controllerName`                                     | Controller name of the GatewayClass. Use a different value per operator instance when several instances share a cluster                                                      | `gateway.envoyproxy.io/gatewayclass-controller` |
| `hoprdOperator.gateway.builtin.enabled`                                    | Create the built-in EnvoyProxy, GatewayClass and Gateway used by the Hoprd nodes                                                                                             | `true`                                          |
| `hoprdOperator.otlp.endpoint`                                              | OTLP endpoint where Hoprd nodes send their telemetry. When empty, telemetry is disabled. An http(s):// URL uses OTLP/HTTP (port 4318), any other value uses gRPC (port 4317) | `""`                                            |
| `hoprdOperator.otlp.signals`                                               | Comma separated list of signals to export (traces,logs,metrics). Empty uses the Hoprd default                                                                                | `""`                                            |
| `hoprdOperator.otlp.metricExportInterval`                                  | Metric export interval in milliseconds, e.g. 15000 or 15000,hopr_session=1000. Empty uses the Hoprd default                                                                  | `""`                                            |
| `hoprdOperator.persistence.size`                                           | Size of the persistence Volume                                                                                                                                               | `500Mi`                                         |
| `hoprdOperator.persistence.storageClassName`                               | Name of the storage class                                                                                                                                                    | `""`                                            |
| `hoprdOperator.webhook.enabled`                                            | Whether to enable the webhook server                                                                                                                                         | `true`                                          |
| `hoprdOperator.webhook.tls.crt`                                            | TLS certificate for the webhook server                                                                                                                                       | `""`                                            |
| `hoprdOperator.webhook.tls.key`                                            | TLS private key for the webhook server                                                                                                                                       | `""`                                            |
| `hoprdOperator.networkPolicy.enabled`                                      | Whether to create a NetworkPolicy allowing                                                                                                                                   | `true`                                          |
| `hoprdOperator.networkPolicy.ingressKonnectivityAgent.namespace`           | Namespace where the konnectivity-agent runs                                                                                                                                  | `kube-system`                                   |
| `hoprdOperator.networkPolicy.ingressKonnectivityAgent.podSelector.k8s-app` | Label identifying the konnectivity-agent pods                                                                                                                                | `konnectivity-agent`                            |
