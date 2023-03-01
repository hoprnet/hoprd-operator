{{/*
Expand the name of the chart.
*/}}
{{- define "hopr-operator.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "hopr-operator.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "hopr-operator.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Default labels
*/}}
{{- define "hopr-operator.labels" -}}
helm.sh/chart: {{ include "hopr-operator.chart" . }}
app.kubernetes.io/component: operator
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/name: {{ .Release.Name }}
app.kubernetes.io/instance: {{ include "hopr-operator.name" . }}

{{- end }}

{{/*
Default labels
*/}}
{{- define "hopr-adminui.labels" -}}
helm.sh/chart: {{ include "hopr-operator.chart" . }}
app.kubernetes.io/component: admin
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/name: {{ .Release.Name }}
app.kubernetes.io/instance: {{ include "hopr-operator.name" . }}
{{- end }}
