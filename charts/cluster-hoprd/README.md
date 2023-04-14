<!--- app-name: Hopr Operator -->

# ClusterHoprd  Chart

This chart packages the creation of a ClusterHoprd


## Parameters

### Common parameters

| Name           | Description                                        | Value |
| -------------- | -------------------------------------------------- | ----- |
| `nameOverride` | String to partially override common.names.fullname | `""`  |

### Cluster Hoprd parameters

| Name                 | Description                                          | Value  |
| -------------------- | ---------------------------------------------------- | ------ |
| `network`            | Network of the ClusterHoprd                          | `""`   |
| `ingress.enabled`    | Whether to create or not the Ingress resource        | `true` |
| `monitoring.enabled` | Whether to create or not the ServiceMonitor resource | `true` |
| `nodes`              | Array of node configuration                          | `[]`   |
