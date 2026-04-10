#!/bin/bash
set -euo pipefail

CHART_NAME="firecracker-manager"

# Check if helm is installed
if ! command -v helm &> /dev/null; then
    echo "Error: helm is not installed"
    exit 1
fi

# Create basic chart structure
helm create $CHART_NAME

# Clean up default templates
rm -rf $CHART_NAME/templates/*
rm -rf $CHART_NAME/values.yaml

# Create values.yaml
cat > $CHART_NAME/values.yaml << 'EOF'
# Default values for firecracker-manager
image:
  repository: 993161092587.dkr.ecr.us-east-1.amazonaws.com/firecracker-manager
  tag: latest
  pullPolicy: Always

nodeSelector:
  node.kubernetes.io/instance-type: c5.metal
  firecracker.vm/enabled: "true"

tolerations:
  - key: "compute.type"
    operator: "Equal"
    value: "metal"
    effect: "NoSchedule"

storage:
  size: 100Gi
  path: /var/lib/firecracker/kernels

service:
  type: ClusterIP
  port: 3000
EOF

# Create namespace template
cat > $CHART_NAME/templates/namespace.yaml << 'EOF'
apiVersion: v1
kind: Namespace
metadata:
  name: {{ .Release.Namespace }}
EOF

# Create daemonset template
cat > $CHART_NAME/templates/daemonset.yaml << 'EOF'
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: {{ .Release.Name }}-manager
  namespace: {{ .Release.Namespace }}
spec:
  selector:
    matchLabels:
      app: {{ .Release.Name }}-manager
  template:
    metadata:
      labels:
        app: {{ .Release.Name }}-manager
    spec:
      nodeSelector:
{{ toYaml .Values.nodeSelector | indent 8 }}
      tolerations:
{{ toYaml .Values.tolerations | indent 8 }}
      containers:
      - name: firecracker-manager
        image: "{{ .Values.image.repository }}:{{ .Values.image.tag }}"
        imagePullPolicy: {{ .Values.image.pullPolicy }}
        ports:
        - containerPort: {{ .Values.service.port }}
          name: http
        securityContext:
          privileged: true
        volumeMounts:
        - name: dev-kvm
          mountPath: /dev/kvm
        - name: vm-data
          mountPath: /var/lib/firecracker
        env:
        - name: NODE_ENV
          value: "production"
      volumes:
      - name: dev-kvm
        hostPath:
          path: /dev/kvm
      - name: vm-data
        hostPath:
          path: /var/lib/firecracker
          type: DirectoryOrCreate
EOF

# Create storage template
cat > $CHART_NAME/templates/storage.yaml << 'EOF'
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: {{ .Release.Name }}-local
provisioner: kubernetes.io/no-provisioner
volumeBindingMode: WaitForFirstConsumer
---
apiVersion: v1
kind: PersistentVolume
metadata:
  name: {{ .Release.Name }}-kernels-pv
spec:
  capacity:
    storage: {{ .Values.storage.size }}
  accessModes:
  - ReadWriteOnce
  persistentVolumeReclaimPolicy: Retain
  storageClassName: {{ .Release.Name }}-local
  local:
    path: {{ .Values.storage.path }}
  nodeAffinity:
    required:
      nodeSelectorTerms:
      - matchExpressions:
        - key: node.kubernetes.io/instance-type
          operator: In
          values:
          - c5.metal
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ .Release.Name }}-kernels-pvc
  namespace: {{ .Release.Namespace }}
spec:
  accessModes:
  - ReadWriteOnce
  resources:
    requests:
      storage: {{ .Values.storage.size }}
  storageClassName: {{ .Release.Name }}-local
EOF

# Create service template
cat > $CHART_NAME/templates/service.yaml << 'EOF'
apiVersion: v1
kind: Service
metadata:
  name: {{ .Release.Name }}-manager
  namespace: {{ .Release.Namespace }}
spec:
  selector:
    app: {{ .Release.Name }}-manager
  ports:
  - port: {{ .Values.service.port }}
    targetPort: {{ .Values.service.port }}
    name: http
  type: {{ .Values.service.type }}
EOF

# Update Chart.yaml
cat > $CHART_NAME/Chart.yaml << 'EOF'
apiVersion: v2
name: firecracker-manager
description: A Helm chart for deploying Firecracker VM Manager on Kubernetes
type: application
version: 0.1.0
appVersion: "1.0.0"
EOF

echo "Helm chart created successfully!"
echo "To install the chart:"
echo "helm install firecracker-manager ./firecracker-manager --namespace firecracker-system --create-namespace"
echo
echo "To uninstall:"
echo "helm uninstall firecracker-manager -n firecracker-system"