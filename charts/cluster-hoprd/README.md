<!--- app-name: Hopr Operator -->

# ClusterHoprd  Chart

This chart packages the creation of a ClusterHoprd


## Parameters

### Common parameters

| Name                                         | Description                                                                              | Value                                               |
| -------------------------------------------- | ---------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `clusterHoprd.enabled`                       | Running status of the nodes                                                              | `true`                                              |
| `clusterHoprd.network`                       | Hoprd Network: rotsee, dufour                                                            | `""`                                                |
| `clusterHoprd.replicas`                      | Number of instances                                                                      | `1`                                                 |
| `clusterHoprd.version`                       | Hoprd node version to run                                                                | `""`                                                |
| `clusterHoprd.funding.enabled`               | Enable cron auto-funding                                                                 | `false`                                             |
| `clusterHoprd.funding.deployerPrivateKey`    | The staking wallet private key used to fund identities                                   | `""`                                                |
| `clusterHoprd.funding.schedule`              | Cron schedule to run auto-funding job.                                                   | `0 1 * * 1`                                         |
| `clusterHoprd.funding.nativeAmount`          | Number of xDai to fund each node                                                         | `0.01`                                              |
| `clusterHoprd.nodes.config`                  | Custom configuration for each node                                                       | `""`                                                |
| `clusterHoprd.nodes.identityPassword`        | Password used by all identities defined bellow                                           | `""`                                                |
| `clusterHoprd.nodes.hoprdApiToken`           | API Token used by all nodes of the cluster                                               | `""`                                                |
| `clusterHoprd.nodes.deployment`              | Deployment spec                                                                          | `{}`                                                |
| `clusterHoprd.nodes.service.type`            | Service Type                                                                             | `ClusterIP`                                         |
| `clusterHoprd.nodes.service.portsAllocation` | Ports allocation                                                                         | `10`                                                |
| `clusterHoprd.nodes.replicateEnvVars`        | Override param clusterHoprd.nodes.envVars with default values provided by Hoprd Operator | `false`                                             |
| `clusterHoprd.nodes.replicateEnvVarsName`    | Namespace/Name of the Secret to replicate when replicateEnvVars is true                  | `hoprd-operator/hoprd-operator-v4-default-env-vars` |
| `clusterHoprd.nodes.envVars`                 | Environment variables to set in Hoprd nodes                                              | `{}`                                                |
| `clusterHoprd.profiling.enabled`             | Enable perf profiling container                                                          | `false`                                             |
| `clusterHoprd.profiling.bucketName`          | GCS Bucket name to store profiling data                                                  | `hoprd-operator-staging`                            |
| `clusterHoprd.profiling.cpu.sampleFrequency` | Frequency of samples per second                                                          | `99`                                                |
| `clusterHoprd.profiling.cpu.sampleDuration`  | Duration of profiling in seconds                                                         | `60`                                                |
| `clusterHoprd.profiling.memory.samples`      | Number of memory samples to generate                                                     | `10`                                                |
| `clusterHoprd.profiling.memory.interval`     | Interval in seconds between memory samples                                               | `15`                                                |
| `clusterHoprd.identities`                    | Map of identities to create                                                              | `{}`                                                |
| `clusterHoprd.logs.download.enabled`         | Enable downloading logs from trusted source                                              | `false`                                             |
| `clusterHoprd.logs.download.snapshotUrl`     | URL to the trusted source of logs                                                        | `""`                                                |
| `clusterHoprd.logs.upload.enabled`           | Enable publishing logs to GCS                                                            | `false`                                             |
| `clusterHoprd.logs.upload.bucketName`        | Name of the bucket to store the logs                                                     | `""`                                                |
| `clusterHoprd.logs.upload.schedule`          | Schedule for uploading logs                                                              | `0 0 * * *`                                         |
| `clusterHoprd.logs.upload.sourceNode`        | Name of the hoprd node deployment use as source                                          | `""`                                                |
| `clusterHoprd.logs.upload.logsFileName`      | Name of the logs file to upload. It should be extension like .tar.xz                     | `""`                                                |
