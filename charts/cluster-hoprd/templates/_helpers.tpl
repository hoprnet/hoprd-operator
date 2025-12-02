{{/*
Expand the name of the chart.
*/}}
{{- define "cluster-hoprd.name" -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- end }}
