#!/bin/bash
set -euo pipefail

# Usage info
usage() {
    echo "Usage: $0 [create|up|down] NODEGROUP_NAME"
    echo "Commands:"
    echo "  create    Create a new metal nodegroup"
    echo "  up        Scale up the nodegroup to 1 node"
    echo "  down      Scale down the nodegroup to 0 nodes"
    echo "Example: $0 create metal-nodes"
    exit 1
}

# Check args
[ $# -lt 2 ] && usage
COMMAND=$1
NODEGROUP=$2

# Extract cluster info from current context
CURRENT_CONTEXT=$(kubectl config current-context)
CLUSTER=$(echo $CURRENT_CONTEXT | cut -d'/' -f2)
REGION=$(echo $CURRENT_CONTEXT | cut -d':' -f4)

echo "Using cluster: $CLUSTER"
echo "Region: $REGION"

# Get ASG name for nodegroup
get_asg_name() {
    aws eks describe-nodegroup \
        --cluster-name "$CLUSTER" \
        --nodegroup-name "$NODEGROUP" \
        --region "$REGION" \
        --query 'nodegroup.resources.autoScalingGroups[0].name' \
        --output text
}

case $COMMAND in
    create)
        echo "Creating metal nodegroup..."
        
        # Get cluster VPC and subnet info
        VPC_ID=$(aws eks describe-cluster \
            --name "$CLUSTER" \
            --region "$REGION" \
            --query 'cluster.resourcesVpcConfig.vpcId' \
            --output text)
        
        SUBNETS=$(aws eks describe-cluster \
            --name "$CLUSTER" \
            --region "$REGION" \
            --query 'cluster.resourcesVpcConfig.subnetIds' \
            --output text)
        
        # Get existing nodegroup to find the role ARN
        EXISTING_NODEGROUPS=$(aws eks list-nodegroups --cluster-name "$CLUSTER" --region "$REGION" --query 'nodegroups[0]' --output text)
        if [ "$EXISTING_NODEGROUPS" == "None" ]; then
            echo "No existing nodegroups found to copy role from"
            exit 1
        fi
        
        NODE_ROLE=$(aws eks describe-nodegroup \
            --cluster-name "$CLUSTER" \
            --nodegroup-name "$EXISTING_NODEGROUPS" \
            --region "$REGION" \
            --query 'nodegroup.nodeRole' \
            --output text)
            
        echo "Using node role: $NODE_ROLE"
        
        # Create nodegroup
        aws eks create-nodegroup \
            --cluster-name "$CLUSTER" \
            --nodegroup-name "$NODEGROUP" \
            --region "$REGION" \
            --node-role "$NODE_ROLE" \
            --subnets $SUBNETS \
            --instance-types c5.metal \
            --scaling-config minSize=0,maxSize=1,desiredSize=0 \
            --disk-size 100 \
            --labels compute.type=metal \
            --taints "key=compute.type,value=metal,effect=NO_SCHEDULE" \
            --tags "k8s.io/cluster-autoscaler/enabled=true,k8s.io/cluster-autoscaler/${CLUSTER}=owned"
            
        echo "Waiting for nodegroup creation to complete..."
        aws eks wait nodegroup-active \
            --cluster-name "$CLUSTER" \
            --nodegroup-name "$NODEGROUP" \
            --region "$REGION"
            
        echo "Metal nodegroup created successfully!"
        ;;
        
    up)
        echo "Scaling up metal node..."
        ASG_NAME=$(get_asg_name)
        
        # Scale up ASG
        aws autoscaling update-auto-scaling-group \
            --auto-scaling-group-name "$ASG_NAME" \
            --min-size 1 \
            --desired-capacity 1 \
            --region "$REGION"
        
        echo "Waiting for node to be ready (this may take 15-30 minutes)..."
        while true; do
            if kubectl get nodes \
                -l eks.amazonaws.com/nodegroup="$NODEGROUP" \
                -o jsonpath='{.items[*].status.conditions[?(@.type=="Ready")].status}' \
                | grep -q "True"; then
                break
            fi
            echo -n "."
            sleep 30
        done
        echo -e "\nMetal node is ready!"
        ;;
        
    down)
        echo "Scaling down metal node..."
        ASG_NAME=$(get_asg_name)
        
        # Scale down ASG
        aws autoscaling update-auto-scaling-group \
            --auto-scaling-group-name "$ASG_NAME" \
            --min-size 0 \
            --desired-capacity 0 \
            --region "$REGION"
            
        echo "Scale down initiated. Node will be terminated shortly."
        ;;
        
    *)
        usage
        ;;
esac