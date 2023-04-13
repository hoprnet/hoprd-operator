{{/*
Expand the name of the chart.
*/}}
{{- define "cluster-hoprd.name" -}}
{{- default .Release.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}
