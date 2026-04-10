#!/bin/bash

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

instanceNumber="$1"
if [[ -z "$instanceNumber" ]]; then
    echo "Please specify an instance number (1, 2, 3) when running this script"
    exit 1
fi

instanceName="arden-ceph-test_$instanceNumber"

ip=$(aws ec2 describe-instances --filters "Name=tag:Name,Values=$instanceName" --query 'Reservations[*].Instances[*].PublicIpAddress' --output text)
if [[ -z "$ip" ]]; then
    echo "Failed to retrieve IP for $instanceName. Aborting."
    exit 1
fi

sshKeyName="arden-ceph-test.pem"
echo "Connecting to $instanceName (IP: $ip)..."
ssh -i "$sshKeyName" ubuntu@"$ip"