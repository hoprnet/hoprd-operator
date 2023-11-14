<!--- app-name: Hopr Operator -->

# ClusterHoprd  Chart

This chart packages the creation of a ClusterHoprd


## Parameters

### Common parameters

| Name           | Description                                        | Value |
| -------------- | -------------------------------------------------- | ----- |
| `nameOverride` | String to partially override common.names.fullname | `""`  |

### Cluster Hoprd parameters

| Name                   | Description                   | Value   |
| ---------------------- | ----------------------------- | ------- |
| `identityPoolName`     | Name of the identity pool     | `""`    |
| `replicas`             | Number of instances           | `1`     |
| `version`              | Hoprd node version to run     | `2.0.2` |
| `enabled`              | Running status of the nodes   | `true`  |
| `deployment.resources` | Deployment resources spec     | `""`    |
| `config`               | Custom configuration of nodes | `""`    |
