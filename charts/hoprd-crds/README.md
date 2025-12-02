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


## Certificate


```
SERVICE=hoprd-operator-webhook
NAMESPACE=hoprd-operator
openssl req -x509 -nodes -days 3650 \
  -newkey rsa:2048 \
  -keyout tls.key \
  -out tls.crt \
  -subj "/CN=${SERVICE}.${NAMESPACE}.svc" \
  -addext "subjectAltName = DNS:${SERVICE}.${NAMESPACE}.svc"
```

## Parameters

### Webhook parameters

| Name                                 | Description                            | Value  |
| ------------------------------------ | -------------------------------------- | ------ |
| `hoprdOperator.webhook.service.name` | Name of the webhook service            | `""`   |
| `hoprdOperator.webhook.service.port` | Port of the webhook service            | `8443` |
| `hoprdOperator.webhook.tls.crt`      | TLS certificate for the webhook server | `""`   |
