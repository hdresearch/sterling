#!/bin/bash
# Setup IAM permissions for proxy EC2 instance to access Secrets Manager

set -e

INSTANCE_ID="${1:-i-04ca5ae14f43367f7}"
REGION="us-east-1"
ROLE_NAME="proxy-secrets-manager-role"
POLICY_NAME="ProxySecretsManagerAccess"

if [ -z "$INSTANCE_ID" ]; then
    echo "Usage: $0 <instance-id>"
    echo "Example: $0 i-04ca5ae14f43367f7"
    exit 1
fi

echo "🔧 Setting up IAM permissions for proxy instance..."

# Check if instance already has a role
EXISTING_ROLE=$(aws ec2 describe-iam-instance-profile-associations \
    --region "$REGION" \
    --filters "Name=instance-id,Values=$INSTANCE_ID" \
    --query 'IamInstanceProfileAssociations[0].IamInstanceProfile.Arn' \
    --output text 2>/dev/null || echo "None")

if [ "$EXISTING_ROLE" != "None" ] && [ "$EXISTING_ROLE" != "" ]; then
    # Extract role name from the instance profile
    EXISTING_ROLE_NAME=$(aws ec2 describe-iam-instance-profile-associations \
        --region "$REGION" \
        --filters "Name=instance-id,Values=$INSTANCE_ID" \
        --query 'IamInstanceProfileAssociations[0].IamInstanceProfile.Arn' \
        --output text | awk -F'/' '{print $NF}')

    echo "✓ Instance already has IAM role: $EXISTING_ROLE_NAME"
    echo "  Adding Secrets Manager policy to existing role..."
    ROLE_NAME="$EXISTING_ROLE_NAME"
else
    echo "Creating new IAM role: $ROLE_NAME"

    # Create trust policy for EC2
    cat > /tmp/trust-policy.json << 'EOF'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "ec2.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
EOF

    # Create the role
    aws iam create-role \
        --role-name "$ROLE_NAME" \
        --assume-role-policy-document file:///tmp/trust-policy.json \
        --description "Role for proxy server to access Secrets Manager" || true

    # Create instance profile
    aws iam create-instance-profile \
        --instance-profile-name "$ROLE_NAME" || true

    # Add role to instance profile
    aws iam add-role-to-instance-profile \
        --instance-profile-name "$ROLE_NAME" \
        --role-name "$ROLE_NAME" || true

    echo "✓ Created IAM role and instance profile"
fi

# Create Secrets Manager policy
echo "Creating Secrets Manager policy..."
cat > /tmp/secrets-policy.json << 'EOF'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "secretsmanager:GetSecretValue",
        "secretsmanager:DescribeSecret"
      ],
      "Resource": [
        "arn:aws:secretsmanager:us-east-1:*:secret:proxy/tls-cert-*",
        "arn:aws:secretsmanager:us-east-1:*:secret:proxy/tls-key-*"
      ]
    }
  ]
}
EOF

# Attach inline policy to role
aws iam put-role-policy \
    --role-name "$ROLE_NAME" \
    --policy-name "$POLICY_NAME" \
    --policy-document file:///tmp/secrets-policy.json

echo "✓ Attached Secrets Manager policy to role"

# Associate role with instance if not already associated
if [ "$EXISTING_ROLE" = "None" ] || [ "$EXISTING_ROLE" = "" ]; then
    echo "Waiting for IAM instance profile to propagate..."
    sleep 5

    echo "Associating IAM role with EC2 instance..."
    # Retry a few times in case of propagation delay
    for i in {1..5}; do
        if aws ec2 associate-iam-instance-profile \
            --region "$REGION" \
            --instance-id "$INSTANCE_ID" \
            --iam-instance-profile "Name=$ROLE_NAME" 2>/dev/null; then
            echo "✓ Associated IAM role with instance"
            break
        else
            if [ $i -lt 5 ]; then
                echo "  Retrying in 3 seconds... (attempt $i/5)"
                sleep 3
            else
                echo "❌ Failed to associate IAM role after 5 attempts"
                echo "   Try running: aws ec2 associate-iam-instance-profile --region $REGION --instance-id $INSTANCE_ID --iam-instance-profile Name=$ROLE_NAME"
                exit 1
            fi
        fi
    done
fi

# Cleanup temp files
rm -f /tmp/trust-policy.json /tmp/secrets-policy.json

echo ""
echo "✅ Setup complete!"
echo ""
echo "Instance: $INSTANCE_ID"
echo "Role: $ROLE_NAME"
echo "Policy: $POLICY_NAME"
echo ""
echo "The proxy container can now access:"
echo "  - proxy/tls-cert"
echo "  - proxy/tls-key"
echo ""
echo "Note: It may take a few seconds for the permissions to propagate."
