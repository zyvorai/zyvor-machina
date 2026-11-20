{{- define "machina.name" -}}
machina
{{- end }}

{{- define "machina.fullname" -}}
{{- printf "%s-%s" .Release.Name (include "machina.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end }}
