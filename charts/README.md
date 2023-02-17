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
