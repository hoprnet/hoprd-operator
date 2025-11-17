<!--- app-name: Hopr Operator -->

# Hopr CRDs Chart

This chart packages the kubernetes Custome resource definitions needed to install Hopr opreator on Kubernetes

## Installing

```console
$ helm repo add hoprd-operator git+https://github.com/hoprnet/hoprd-operator@charts?ref=master
$ helm install hoprd-crd hoprd-operator/hoprd-crd
```

## Uninstalling the Chart

To uninstall/delete the release:

```console
helm delete hoprd-crd
```
