#!/bin/bash
# Upload proxy certificates to AWS Secrets Manager

set -e

REGION="us-east-1"
CERT_FILE="${1:-/home/ubuntu/src/chelsea/proxy-cert.pem}"
KEY_FILE="${2:-/home/ubuntu/src/chelsea/proxy-key.pem}"

echo "📦 Uploading certificates to AWS Secrets Manager..."

# Check if files exist
if [ ! -f "$CERT_FILE" ]; then
    echo "❌ Certificate file not found: $CERT_FILE"
    exit 1
fi

if [ ! -f "$KEY_FILE" ]; then
    echo "❌ Key file not found: $KEY_FILE"
    exit 1
fi

# Upload cert
echo "Uploading certificate..."
aws secretsmanager create-secret \
    --name proxy/tls-cert \
    --description "Proxy TLS certificate" \
    --secret-string file://"$CERT_FILE" \
    --region "$REGION" 2>/dev/null || \
aws secretsmanager update-secret \
    --secret-id proxy/tls-cert \
    --secret-string file://"$CERT_FILE" \
    --region "$REGION"

# Upload key
echo "Uploading private key..."
aws secretsmanager create-secret \
    --name proxy/tls-key \
    --description "Proxy TLS private key" \
    --secret-string file://"$KEY_FILE" \
    --region "$REGION" 2>/dev/null || \
aws secretsmanager update-secret \
    --secret-id proxy/tls-key \
    --secret-string file://"$KEY_FILE" \
    --region "$REGION"

echo "✅ Certificates uploaded successfully!"
echo ""
echo "Secrets created:"
echo "  - proxy/tls-cert"
echo "  - proxy/tls-key"
echo ""
echo "Next steps:"
echo "  1. Ensure proxy container has IAM permissions to read these secrets"
echo "  2. Update proxy code to fetch certs from Secrets Manager"
