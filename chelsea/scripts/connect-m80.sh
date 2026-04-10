#!/bin/bash

# Get the instance IP address
INSTANCE_IP=$(aws ec2 describe-instances \
    --filters "Name=tag:Name,Values=m80-dev" \
    --query 'Reservations[*].Instances[*].PublicIpAddress' \
    --output text)

if [ -z "$INSTANCE_IP" ]; then
    echo "Error: Could not find public IP for m80-dev"
    exit 1
fi

# SSH into the instance
# Note: Replace 'ubuntu' with your instance's username if different
# Replace path/to/your-key.pem with your actual key path
ssh -i ./firecracker.pem ubuntu@$INSTANCE_IP 