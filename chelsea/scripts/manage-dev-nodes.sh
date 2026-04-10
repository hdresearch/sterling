#!/bin/bash

set -euo pipefail

# Options for provisioning
INSTANCE_TYPE="c8i.2xlarge"
VOLUME_SIZE="512"
# Nested virtualization is a new feature as of Feb 16 2026; requires AWS CLI update, and applies to m8i, c8i, and r8i instances.
CPU_OPTIONS='{"NestedVirtualization":"enabled"}'

# Misc configuration
SWAP_SIZE="64G"

usage() {
    printf "\n"
    echo "Usage: $0 [provision|stop|start|terminate] GITHUB_USERNAME"
    printf "\n"
    echo "Commands:"
    echo "  provision    Setup an EC2 node for Chelsea Development (requires GITHUB_TOKEN env var)"
    echo "  stop         Stop the node"
    echo "  start        Start the node"
    echo "  terminate    Kill the node and release all AWS resources"
    echo "  help THING   Get additional help on THING"
    echo "Example: $0 provision shofetim"
    printf "\n\n"
    echo "Assumptions:"
    # shellcheck disable=SC2016
    echo '- You have a Github auth token with at least repo scope (`help token`)'
    # shellcheck disable=SC2016
    echo '- The AWS CLI is available (`help CLI`)'
    # shellcheck disable=SC2016
    echo '- AWS CLI authentication is already setup (`help auth`)'
    # shellcheck disable=SC2016
    echo '- The shared-dev-key.pem file is available in this script folder (`help key`)'
    printf "\n"
    echo "For support hit up Jordan"
    printf "\n"
    exit
}

cli() {
    printf "\n"
    echo "Amazon provides full installation instructions here:"
    echo "https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html"
    printf "\n"
    echo "If you are on MacOS, this should do it"
    echo 'curl "https://awscli.amazonaws.com/AWSCLIV2.pkg" -o "AWSCLIV2.pkg'
    echo 'sudo installer -pkg AWSCLIV2.pkg -target /'
    printf "\n"
}

auth() {
    printf "\n"
    echo "Amazon provides instructions for how to setup authentication here:"
    echo "https://docs.aws.amazon.com/cli/latest/userguide/cli-authentication-user.html#cli-authentication-user-get"
    printf "\n"
}

token() {
    printf "\n"
    echo "The ansible scripts that are used to setup the node use a "
    echo "Github token to find the current release of Chelsea"
    echo "To obtain a token follow the instructions here:"
    echo "https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-personal-access-token-classic"
    printf "\n"
}

key() {
    printf "\n"
    echo "The key is shared in the secrets file in the sytem repo"
    echo "Obtain it from there, and set the permissions to 600"
    printf "\n"
}

check_account() {
    account_id=$(aws sts get-caller-identity --query "Account" --output text)
    if [ "$account_id" != "993161092587" ]; then
        echo "AWS credentials are for the wrong account!"
        exit
    fi
}

rexec() {
    ssh -T -S /tmp/control -i shared-dev-key.pem -o ForwardAgent=yes ubuntu@"$ip" "$@"
}

# Check args
[ $# -lt 2 ] && usage
COMMAND=$1
GITHUB_USERNAME=$2

case $COMMAND in
    help)
        case $GITHUB_USERNAME in
            cli)
                cli
            ;;
            auth)
                auth
            ;;
            token)
                token
            ;;
            key)
                key
            ;;
            *)
                usage
            ;;
        esac
    ;;
    provision)
        check_account

        if [ -z "$GITHUB_TOKEN" ]; then
            echo "Error: GITHUB_TOKEN environment variable is not set."
            echo "Please set GITHUB_TOKEN before running provision."
            exit 1
        fi

        printf "Provisioning new dev node...\n"
        echo "This will take some time (minutes), please be patient"
        echo "Please insure that the shared dev key is available at shared-dev-key.pem"

        instance_id=$(aws ec2 describe-instances \
                          --filters 'Name=tag:Name,Values=dev-'"$GITHUB_USERNAME" \
                          --query 'Reservations[0].Instances[0].InstanceId' \
                          --output text)

        if [ "$instance_id" = 'None' ]; then
            instance_id=$(aws ec2 run-instances \
                              --image-id "ami-0360c520857e3138f" \
                              --instance-type "$INSTANCE_TYPE" \
                              --key-name "Shared-Dev-Key" \
                              --block-device-mappings '{"DeviceName":"/dev/sda1","Ebs":{"Encrypted":false,"DeleteOnTermination":true,"Iops":3000,"SnapshotId":"snap-05ebf17f9a0bc77de","VolumeSize":'"$VOLUME_SIZE"',"VolumeType":"gp3","Throughput":500}}' \
                              --network-interfaces '{"SubnetId":"subnet-0fb22e01f2704c71c","AssociatePublicIpAddress":true,"DeviceIndex":0,"Groups":["sg-001937847753e21ec"]}' \
                              --iam-instance-profile Arn="arn:aws:iam::993161092587:instance-profile/VersDev" \
                              --tag-specifications '{"ResourceType":"instance","Tags":[{"Key":"Name","Value":"dev-'"$GITHUB_USERNAME"'"}]}' \
                              --metadata-options '{"HttpEndpoint":"enabled","HttpPutResponseHopLimit":2,"HttpTokens":"required"}' \
                              --private-dns-name-options '{"HostnameType":"ip-name","EnableResourceNameDnsARecord":false,"EnableResourceNameDnsAAAARecord":false}' \
                              --cpu-options "$CPU_OPTIONS" \
                              --count "1" \
                              --query 'Instances[0].InstanceId' \
                              --output text)

        fi

        aws ec2 start-instances --instance-ids "$instance_id"
        aws ec2 wait instance-status-ok --instance-ids "$instance_id"

        ip=$(aws ec2 describe-instances \
                   --instance-ids "$instance_id" \
                   --query 'Reservations[0].Instances[0].PublicIpAddress' \
                   --output text)

        ssh-keyscan "$ip" >> ~/.ssh/known_hosts
        rexec -fN -M
        rexec sudo apt-get update
        rexec sudo apt-get -y install ansible
        rexec 'curl -sSf https://sh.rustup.rs | sh -s -- -y'

        # Miscellaneous dev dependencies
        rexec sudo apt-get install -y entr sqlite3 tmux clang gcc make libssl-dev ceph-common mkcert sshpass gh musl-tools

        # Clone the source
        rexec mkdir -p src
        set +e # it is ok for these to fail if they already exist
        rexec git clone https://"$GITHUB_USERNAME":"$GITHUB_TOKEN"@github.com/hdresearch/vers-lb.git src/vers-lb
        rexec git clone https://"$GITHUB_USERNAME":"$GITHUB_TOKEN"@github.com/hdresearch/chelsea.git src/chelsea
        set -e

        # Set up swap
        rexec sudo fallocate -l $SWAP_SIZE /swap
        rexec sudo chmod 600 /swap
        rexec sudo mkswap /swap
        rexec 'echo -e "/swap\tswap\tswap\tsw\t0\t0" | sudo tee -a /etc/fstab >/dev/null'
        rexec 'sudo systemctl daemon-reload && sudo systemctl start swap.swap'

        # Setup via Ansible
        rexec 'export GITHUB_TOKEN='"$GITHUB_TOKEN"' && ADMIN_API_KEY=speakfriendandenter && cd src/vers-lb/packages/ansible-setup/ && ansible-playbook -i 127.0.0.1, --connection=local bootstrap-chelsea.yml'
        rm /tmp/control
        echo "Node provisioned successfully!"
        echo "IP is: $ip"
        echo "Login with"
        echo "ssh -i shared-dev-key.pem ubuntu@$ip"
        echo "ADMIN_API_KEY is speakfriendandenter"
        ;;
        
    stop)
        check_account
        echo "Stopping dev node..."
        instance_id=$(aws ec2 describe-instances \
                            --filters 'Name=tag:Name,Values=dev-'"$GITHUB_USERNAME" \
                            --query 'Reservations[0].Instances[0].InstanceId' \
                            --output text)
        aws ec2 stop-instances --instance-ids "$instance_id"
        # aws ec2 wait instance-stopped --instance-ids "$instance_id"
        echo "Stop command issued. Shutdown general takes >5 min, so not waiting."
        ;;

    start)
        check_account
        echo "Starting dev node..."
        instance_id=$(aws ec2 describe-instances \
                            --filters 'Name=tag:Name,Values=dev-'"$GITHUB_USERNAME" \
                            --query 'Reservations[0].Instances[0].InstanceId' \
                            --output text)
        aws ec2 start-instances --instance-ids "$instance_id"
        aws ec2 wait instance-running --instance-ids "$instance_id"
        ip=$(aws ec2 describe-instances \
                 --instance-ids "$instance_id" \
                 --query 'Reservations[0].Instances[0].PublicIpAddress' \
                 --output text)
        echo "Node started."
        echo "ssh -i shared-dev-key.pem ubuntu@$ip"
        ;;

    terminate)
        check_account
        echo "Terminating node..."
        echo "This can take some time."
        instance_id=$(aws ec2 describe-instances \
                            --filters 'Name=tag:Name,Values=dev-'"$GITHUB_USERNAME" \
                            --query 'Reservations[0].Instances[0].InstanceId' \
                            --output text)
        aws ec2 terminate-instances --instance-ids "$instance_id"
        aws ec2 wait instance-terminated --instance-ids "$instance_id"
        echo "Node terminated."
        ;;
        
    *)
        usage
        ;;
esac
