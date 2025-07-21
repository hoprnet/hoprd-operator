<!--- app-name: Hopr Operator -->

# ClusterHoprd  Chart

This chart packages the creation of a ClusterHoprd


## Parameters

### Common parameters

| Name                                 | Description                                                                     | Value                     |
| ------------------------------------ | ------------------------------------------------------------------------------- | ------------------------- |
| `nameOverride`                       | String to partially override common.names.fullname                              | `""`                      |
| `wallet.deployerPrivateKey`          | The staking wallet private key used to create identities and to auto fund nodes | `""`                      |
| `wallet.identityPassword`            | Password used by all identities defined bellow                                  | `""`                      |
| `wallet.hoprdApiToken`               | API Token used by all nodes of the cluster                                      | `""`                      |
| `network`                            | Hoprd Network: rotsee, dufour                                                   | `""`                      |
| `identityPool.funding.enabled`       | Enable cron auto-funding                                                        | `false`                   |
| `identityPool.funding.schedule`      | Cron schedule to run auto-funding job.                                          | `0 1 * * 1`               |
| `identityPool.funding.nativeAmount`  | Number of xDai to fund each node                                                | `0.01`                    |
| `identities`                         | Map of identities to create                                                     | `{}`                      |
| `replicas`                           | Number of instances                                                             | `1`                       |
| `version`                            | Hoprd node version to run                                                       | `""`                      |
| `enabled`                            | Running status of the nodes                                                     | `true`                    |
| `supportedRelease`                   | The kind of supported release <saint-louis>                                     | `""`                      |
| `forceIdentityName`                  | Forces identity names to be set in child Hopd resources                         | `false`                   |
| `deployment`                         | Deployment spec                                                                 | `{}`                      |
| `portsAllocation`                    | Ports allocation                                                                | `10`                      |
| `service.type`                       | Service Type                                                                    | `ClusterIP`               |
| `config`                             | Custom configuration of nodes                                                   | `""`                      |
| `replicateDefaultEnvSecret.enabled`  | Enable secret replication                                                       | `true`                    |
| `defaultHoprdEnvVars.HOPRD_PROVIDER` | RPC Provider to use by default to all hoprd nodes                               | `https://gnosis.drpc.org` |
