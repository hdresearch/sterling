#!/bin/bash

# !! PLEASE NOTE !!
# As of November 10 on the branch `arden/reconfig`, proxy returns 200s even in cases where it shouldn't, such as a 404 from the orchestrator.
# If a response comes back empty, it is quite possibly an orch error being masked.

# Configuration
# Actual location of host
API_HOST="204.0.0.2:443"
AUTH_TOKEN="ef90fd52-66b5-47e7-b7dc-e73c4381028fbfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c"
# Host header for API calls
HOST_HEADER="api.vers.sh"

ORCH_HOST="[fd00:fe11:deed::ffff]:8090"
ORCH_ADMIN_API_KEY="3114e635-285c-4c83-be5c-9a68542f6d25"

# Seeded user/org in dev DB
DEFAULT_USER_ID="9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9"
DEFAULT_ORG_ID="2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d"

# Proxy TLS testing configuration
PROXY_HOST="${PROXY_HOST:-204.0.0.2}"
PROXY_PORT=443
VM_BASE_DOMAIN="${VM_BASE_DOMAIN:-vm.vers.sh}"

set -euo pipefail

BASE_URL="https://${API_HOST}/api/v1"

usage() {
    cat << EOF
Usage: $(basename "$0") <command> [options]

Commands:
  list                                                List all VMs
  run-commit <commit-id>                              Restore VM from commit
  run-tag <tag-name>                                  Restore VM from tag
  new [options]                                       Create new VM
  exec <vm-id> [exec-options] -- <command ...>        Run a command inside a VM
  exec-logs <vm-id> [log-options]                     Retrieve exec log entries
  delete <vm-id> [--skip-wait-boot]                   Delete a VM
  branch <vm-id> [--count <num>]                      Create a branch VM
  branch-tag <tag-name> [--count <num>]               Create a branch VM from tag
  commit <vm-id> [--keep-paused] [--skip-wait-boot]   Commit a VM
  pause <vm-id> [--skip-wait-boot]                    Pause a VM
  resume <vm-id> [--skip-wait-boot]                   Resume a VM
  status <vm-id>                                      Get the status of a VM
  ssh-key <vm-id>                                     Get the SSH key of a VM
  resize-disk <vm-id> <size-mib> [--skip-wait-boot]   Resize VM disk to new size
  label <vm-id> [--label key=value ...]               Set labels on an existing VM

Commit Commands:
  commits [options]                                   List all commits
  commit-delete <commit-id>                           Delete a commit you own

Tag Management Commands:
  tag-create [options]                                Create a new tag
  tag-list                                            List all tags
  tag-get <tag-name>                                  Get tag details
  tag-update <tag-name> [options]                     Update a tag (move or change description)
  tag-delete <tag-name>                               Delete a tag

Domain Management Commands:
  domain-create [options]                             Create a custom domain for a VM
  domain-list [options]                               List domains (optionally filter by VM)
  domain-get <domain-id>                              Get domain details
  domain-delete <domain-id>                           Delete a domain
  connect <vm-id>                                     SSH connection through proxy to VM

Base Image Commands:
  images [options]                                    List all base images
  image-create [options]                              Create a new base image
  image-upload [options]                              Upload a tarball to create a base image
  image-status <image-name> [--poll]                  Get base image creation status
  image-delete <image-id>                             Delete a base image

Admin Commands:
  generate-api-key [--user <user-id>] [--org <org-id>] [--label <label>]   Generate a new API key (defaults to DEFAULT_USER/ORG_ID)
  sleep <vm-id> [--skip-wait-boot]                                       Sleep a VM
  wake <vm-id> [--node <node-id>]                                        Wake a sleeping VM
  move <vm-id> [--node <node-id>] [--skip-wait-boot]                     Move a VM to a new node

Proxy TLS Testing Commands (test VM routing through proxy):
  vm-request-uuid <vm-id> [path]                      Send HTTPS request to VM using UUID subdomain
  vm-request-custom <domain> [path]                   Send HTTPS request to VM using custom domain
  acme-challenge <domain> <token>                     Test ACME HTTP-01 challenge endpoint

  Note: 500 responses indicate successful proxy routing (VM may still be booting)

Create-root options:
  --vcpu <count>                   Number of vCPUs
  --mem <size-mib>                 Memory size in MiB
  --fs <size-mib>                  Filesystem size in MiB
  --kernel <name>                  Kernel name (default: default.bin)
  --image <name>                   Image name (default: default)
  --label <key=value>              Add a label (can be specified multiple times)
  --wait-boot                      Wait for VM to finish booting before returning

Common options:
  --skip-wait-boot                 Don't wait for VM to finish booting (error if still booting)

Exec options:
  --tty                            Request a TTY for the exec session
  --stdin <text>                   Provide stdin content
  --timeout <seconds>              Override the exec timeout (seconds)
  --skip-wait-boot                 Return immediately if VM is still booting
  --                               Treat everything afterwards as the command (useful when command args start with --)

Exec log options:
  --offset <bytes>                 Start reading logs from byte offset
  --max-entries <n>                Maximum number of entries to return
  --stream <stdout|stderr>         Filter logs by stream
  --skip-wait-boot                 Return immediately if VM is still booting

Commits list options:
  --limit <count>                  Maximum number of commits to return (default: 50, max: 100)
  --offset <count>                 Number of commits to skip (default: 0)

Images list options:
  --limit <count>                  Maximum number of images to return (default: 50, max: 100)
  --offset <count>                 Number of images to skip (default: 0)

Image-create options:
  --name <name>                    Image name (required)
  --docker <image-ref>             Create from Docker image (e.g., "ubuntu:24.04")
  --s3-bucket <bucket>             S3 bucket name (use with --s3-key)
  --s3-key <key>                   S3 object key (use with --s3-bucket)
  --size <size-mib>                Image size in MiB (default: 512)
  --description <text>             Optional description

Image-upload options:
  --name <name>                    Image name (required)
  --file <path>                    Path to tarball file (required)
  --size <size-mib>                Image size in MiB (default: 512)

Tag-create options:
  --name <name>                    Tag name (required)
  --commit <commit-id>             Commit ID to tag (required)
  --description <text>             Optional description

Tag-update options:
  --commit <commit-id>             Move tag to new commit
  --description <text>             Update tag description
  (At least one option is required)

Domain-create options:
  --vm <vm-id>                     VM ID to attach domain to (required)
  --domain <domain-name>           Domain name (required, e.g., example.com)

Domain-list options:
  --vm <vm-id>                     Filter domains by VM ID (optional)

Examples:
  $(basename "$0") list
  $(basename "$0") run-commit 123e4567-e89b-12d3-a456-426614174000
  $(basename "$0") run-tag production
  $(basename "$0") new --vcpu 4 --mem 2048 --fs 10240 --wait-boot
  $(basename "$0") new --label env=prod --label team=platform
  $(basename "$0") label abc12345-6789-0def-1234-56789abcdef0 --label env=staging
  $(basename "$0") delete abc12345-6789-0def-1234-56789abcdef0 --skip-wait-boot
  $(basename "$0") branch abc12345-6789-0def-1234-56789abcdef0 --count 3
  $(basename "$0") branch-tag production --count 3
  $(basename "$0") commit abc12345-6789-0def-1234-56789abcdef0 --keep-paused --skip-wait-boot
  $(basename "$0") resize abc12345-6789-0def-1234-56789abcdef0 --fs 20480
  $(basename "$0") commits
  $(basename "$0") commits --limit 10 --offset 0
  $(basename "$0") commit-delete 123e4567-e89b-12d3-a456-426614174000
  $(basename "$0") tag-create --name production --commit 123e4567-e89b-12d3-a456-426614174000 --description "Production release"
  $(basename "$0") tag-list
  $(basename "$0") tag-get production
  $(basename "$0") tag-update production --commit 234e5678-e89b-12d3-a456-426614174001
  $(basename "$0") tag-delete staging
  $(basename "$0") domain-create --vm abc12345-6789-0def-1234-56789abcdef0 --domain example.com
  $(basename "$0") domain-list
  $(basename "$0") domain-list --vm abc12345-6789-0def-1234-56789abcdef0
  $(basename "$0") domain-get ef6b51c1-3f1e-4a8f-93ea-08e4c9ff7678
  $(basename "$0") domain-delete ef6b51c1-3f1e-4a8f-93ea-08e4c9ff7678
  $(basename "$0") images
  $(basename "$0") images --limit 10 --offset 0
  $(basename "$0") image-create --name my-ubuntu --docker ubuntu:24.04 --size 1024
  $(basename "$0") image-create --name my-s3-image --s3-bucket my-bucket --s3-key images/rootfs.tar
  $(basename "$0") image-upload --name my-uploaded-image --file /path/to/rootfs.tar --size 1024
  $(basename "$0") image-status my-ubuntu
  $(basename "$0") image-delete 123e4567-e89b-12d3-a456-426614174000
  $(basename "$0") vm-request-uuid 550e8400-e29b-41d4-a716-446655440000
  $(basename "$0") vm-request-custom example.com /api/health
  $(basename "$0") acme-challenge example.com test-token-123
  $(basename "$0") sleep abc12345-6789-0def-1234-56789abcdef0
  $(basename "$0") wake abc12345-6789-0def-1234-56789abcdef0
  $(basename "$0") wake abc12345-6789-0def-1234-56789abcdef0 --node 550e8400-e29b-41d4-a716-446655440000
  $(basename "$0") move abc12345-6789-0def-1234-56789abcdef0
  $(basename "$0") move abc12345-6789-0def-1234-56789abcdef0 --node 550e8400-e29b-41d4-a716-446655440000
EOF
    exit 1
}

admin_api_call() {
    local method="$1"
    local endpoint="$2"
    local data="${3:-}"

    local curl_args=(
        -X "$method"
        -H "Authorization: Bearer ${ORCH_ADMIN_API_KEY}"
        -H "Content-Type: application/json"
        -sS -w "\n%{http_code}"
    )

    if [ -n "$data" ]; then
        curl_args+=(-d "$data")
    fi

    response=$(curl "${curl_args[@]}" "http://${ORCH_HOST}/api/v1${endpoint}")

    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$http_code" -ge 400 ]; then
        echo "Error: HTTP $http_code" >&2
        echo "$body" | jq -r '.error // .' >&2
        exit 1
    fi

    echo "$body" | jq '.' 2>/dev/null || echo "$body"
}

api_call() {
    local method="$1"
    local endpoint="$2"
    local data="${3:-}"

    local curl_args=(
        -X "$method"
        -H "Authorization: Bearer ${AUTH_TOKEN}"
        -H "Host: ${HOST_HEADER}"
        -H "Content-Type: application/json"
        -sS -w "\n%{http_code}"
        --resolve ${HOST_HEADER}:443:204.0.0.2
    )

    if [ -n "$data" ]; then
        curl_args+=(-d "$data")
    fi

    # Sudo needed because the generated cert is owned by root
    response=$(sudo curl "${curl_args[@]}" "https://${HOST_HEADER}:443/api/v1${endpoint}")
    echo $response

    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$http_code" -ge 400 ]; then
        echo "Error: HTTP $http_code" >&2
        echo "$body" | jq -r '.error // .' >&2
        exit 1
    fi

    echo "$body" | jq '.' 2>/dev/null || echo "$body"
}

build_command_json() {
    if [ "$#" -eq 0 ]; then
        echo ""
        return 1
    fi

    printf '%s\n' "$@" | jq -R -s 'split("\n")[:-1]'
}

case "${1:-}" in
    list)
        api_call GET "/vms"
        ;;

    run-commit)
        [ -z "${2:-}" ] && usage
        commit_id="$2"
        api_call POST "/vm/from_commit" "{\"commit_id\":\"$commit_id\"}"
        ;;

    run-tag)
        [ -z "${2:-}" ] && usage
        tag_name="$2"
        api_call POST "/vm/from_commit" "{\"tag_name\":\"$tag_name\"}"
        ;;

    new)
        shift
        parts=()
        labels=()
        query_params=""

        while [ $# -gt 0 ]; do
            case "$1" in
                --vcpu) parts+=("\"vcpu_count\":$2"); shift 2 ;;
                --mem) parts+=("\"mem_size_mib\":$2"); shift 2 ;;
                --fs) parts+=("\"fs_size_mib\":$2"); shift 2 ;;
                --kernel) parts+=("\"kernel_name\":\"$2\""); shift 2 ;;
                --image) parts+=("\"image_name\":\"$2\""); shift 2 ;;
                --label) labels+=("$2"); shift 2 ;;
                --wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?wait_boot=true"
                    else
                        query_params="${query_params}&wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        # Build labels JSON if any labels were provided
        if [ ${#labels[@]} -gt 0 ]; then
            labels_json=$(printf '%s\n' "${labels[@]}" | jq -R 'split("=") | {(.[0]): .[1]}' | jq -s 'add')
            parts+=("\"labels\":$labels_json")
        fi

        IFS=,
        config="{\"vm_config\":{${parts[*]}}}"
        unset IFS

        api_call POST "/vm/new_root${query_params}" "$config"
        ;;

    exec)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        tty=false
        skip_wait_boot=false
        stdin_payload=""
        timeout_secs=""
        command_args=()

        while [ $# -gt 0 ]; do
            case "$1" in
                --tty)
                    tty=true
                    shift
                    ;;
                --skip-wait-boot)
                    skip_wait_boot=true
                    shift
                    ;;
                --stdin)
                    stdin_payload="${2:-}"
                    [ -z "$stdin_payload" ] && { echo "Error: --stdin requires a value" >&2; usage; }
                    shift 2
                    ;;
                --timeout)
                    timeout_secs="${2:-}"
                    [ -z "$timeout_secs" ] && { echo "Error: --timeout requires a value" >&2; usage; }
                    shift 2
                    ;;
                --)
                    shift
                    command_args=("$@")
                    break
                    ;;
                *)
                    command_args=("$@")
                    break
                    ;;
            esac
        done

        if [ "${#command_args[@]}" -eq 0 ]; then
            echo "Error: exec requires a command to run" >&2
            usage
        fi

        command_json=$(build_command_json "${command_args[@]}") || {
            echo "Failed to encode command" >&2
            exit 1
        }

        jq_args=(-n --argjson command "$command_json" --argjson tty "$([ "$tty" = true ] && echo true || echo false)")
        jq_expr='{command:$command, tty:$tty}'

        if [ -n "$stdin_payload" ]; then
            jq_args+=(--arg stdin "$stdin_payload")
            jq_expr="$jq_expr + {stdin:$stdin}"
        fi

        if [ -n "$timeout_secs" ]; then
            jq_args+=(--arg timeout "$timeout_secs")
            jq_expr="$jq_expr + {timeout_secs:($timeout|tonumber)}"
        fi

        request=$(jq "${jq_args[@]}" "$jq_expr")

        query_params=""
        if [ "$skip_wait_boot" = true ]; then
            query_params="?skip_wait_boot=true"
        fi

        api_call POST "/vm/$vm_id/exec${query_params}" "$request"
        ;;

    exec-logs)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        offset=""
        max_entries=""
        stream=""
        skip_wait_boot=false

        while [ $# -gt 0 ]; do
            case "$1" in
                --offset)
                    offset="$2"
                    shift 2
                    ;;
                --max-entries)
                    max_entries="$2"
                    shift 2
                    ;;
                --stream)
                    stream="$2"
                    shift 2
                    ;;
                --skip-wait-boot)
                    skip_wait_boot=true
                    shift
                    ;;
                *)
                    echo "Unknown option: $1" >&2
                    usage
                    ;;
            esac
        done

        query_params=""
        sep="?"
        if [ -n "$offset" ]; then
            query_params="${query_params}${sep}offset=$offset"
            sep="&"
        fi
        if [ -n "$max_entries" ]; then
            query_params="${query_params}${sep}max_entries=$max_entries"
            sep="&"
        fi
        if [ -n "$stream" ]; then
            query_params="${query_params}${sep}stream=$stream"
            sep="&"
        fi
        if [ "$skip_wait_boot" = true ]; then
            query_params="${query_params}${sep}skip_wait_boot=true"
        fi

        api_call GET "/vm/$vm_id/exec/logs${query_params}"
        ;;

    delete)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --skip-wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?skip_wait_boot=true"
                    else
                        query_params="${query_params}&skip_wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call DELETE "/vm/$vm_id${query_params}"
        ;;

    branch)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --count)
                    if [ -z "$query_params" ]; then
                        query_params="?count=$2"
                    else
                        query_params="${query_params}&count=$2"
                    fi
                    shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call POST "/vm/$vm_id/branch${query_params}"
        ;;

    branch-tag)
        [ -z "${2:-}" ] && usage
        tag_name="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --count)
                    if [ -z "$query_params" ]; then
                        query_params="?count=$2"
                    else
                        query_params="${query_params}&count=$2"
                    fi
                    shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call POST "/vm/branch/by_tag/$tag_name${query_params}"
        ;;

    commit)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --keep-paused)
                    if [ -z "$query_params" ]; then
                        query_params="?keep_paused=true"
                    else
                        query_params="${query_params}&keep_paused=true"
                    fi
                    shift
                    ;;
                --skip-wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?skip_wait_boot=true"
                    else
                        query_params="${query_params}&skip_wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call POST "/vm/$vm_id/commit${query_params}"
        ;;

    pause)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --skip-wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?skip_wait_boot=true"
                    else
                        query_params="${query_params}&skip_wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call PATCH "/vm/$vm_id/state${query_params}" '{"state":"Paused"}'
        ;;

    resume)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --skip-wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?skip_wait_boot=true"
                    else
                        query_params="${query_params}&skip_wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call PATCH "/vm/$vm_id/state${query_params}" '{"state":"Running"}'
        ;;

    status)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        api_call GET "/vm/$vm_id/status"
        ;;

    ssh-key)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        api_call GET "/vm/$vm_id/ssh_key"
        ;;

    resize-disk)
        [ -z "${2:-}" ] || [ -z "${3:-}" ] && { echo "Usage: resize-disk <vm-id> <size-mib> [--skip-wait-boot]"; usage; }
        vm_id="$2"
        size_mib="$3"
        shift 3
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --skip-wait-boot)
                    if [ -z "$query_params" ]; then
                        query_params="?skip_wait_boot=true"
                    else
                        query_params="${query_params}&skip_wait_boot=true"
                    fi
                    shift
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call PATCH "/vm/$vm_id/disk${query_params}" "{\"fs_size_mib\":$size_mib}"
        ;;

    commits)
        shift
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --limit)
                    if [ -z "$query_params" ]; then
                        query_params="?limit=$2"
                    else
                        query_params="${query_params}&limit=$2"
                    fi
                    shift 2
                    ;;
                --offset)
                    if [ -z "$query_params" ]; then
                        query_params="?offset=$2"
                    else
                        query_params="${query_params}&offset=$2"
                    fi
                    shift 2
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call GET "/commits${query_params}"
        ;;

    connect)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        rm -f /tmp/key
        api_call GET "/vm/$vm_id/ssh_key" | jq -r .ssh_private_key > /tmp/key
        chmod 400 /tmp/key
        ssh -i /tmp/key \
            -o StrictHostKeyChecking=accept-new \
            -o ProxyCommand="openssl s_client -quiet -connect 204.0.0.2:443 -servername $vm_id.vm.vers.sh" \
            root@$vm_id.vm.vers.sh
        ;;

    commit-delete)
        [ -z "${2:-}" ] && usage
        commit_id="$2"
        api_call DELETE "/commits/$commit_id"
        ;;

    images)
        shift
        query_params=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --limit)
                    if [ -z "$query_params" ]; then
                        query_params="?limit=$2"
                    else
                        query_params="${query_params}&limit=$2"
                    fi
                    shift 2
                    ;;
                --offset)
                    if [ -z "$query_params" ]; then
                        query_params="?offset=$2"
                    else
                        query_params="${query_params}&offset=$2"
                    fi
                    shift 2
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        api_call GET "/images${query_params}"
        ;;

    image-create)
        shift
        image_name=""
        docker_ref=""
        s3_bucket=""
        s3_key=""
        size_mib="512"
        description=""

        while [ $# -gt 0 ]; do
            case "$1" in
                --name) image_name="$2"; shift 2 ;;
                --docker) docker_ref="$2"; shift 2 ;;
                --s3-bucket) s3_bucket="$2"; shift 2 ;;
                --s3-key) s3_key="$2"; shift 2 ;;
                --size) size_mib="$2"; shift 2 ;;
                --description) description="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ -z "$image_name" ] && { echo "Error: --name is required" >&2; usage; }

        # Build the source JSON based on provided options
        if [ -n "$docker_ref" ]; then
            source_json="{\"type\":\"docker\",\"image_ref\":\"$docker_ref\"}"
        elif [ -n "$s3_bucket" ] && [ -n "$s3_key" ]; then
            source_json="{\"type\":\"s3\",\"bucket\":\"$s3_bucket\",\"key\":\"$s3_key\"}"
        else
            echo "Error: Must specify either --docker or both --s3-bucket and --s3-key" >&2
            usage
        fi

        # Build the request JSON
        request="{\"image_name\":\"$image_name\",\"source\":$source_json,\"size_mib\":$size_mib"
        if [ -n "$description" ]; then
            request="$request,\"description\":\"$description\""
        fi
        request="$request}"

        api_call POST "/images/create" "$request"
        ;;

    image-upload)
        shift
        image_name=""
        file_path=""
        size_mib="512"

        while [ $# -gt 0 ]; do
            case "$1" in
                --name) image_name="$2"; shift 2 ;;
                --file) file_path="$2"; shift 2 ;;
                --size) size_mib="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ -z "$image_name" ] && { echo "Error: --name is required" >&2; usage; }
        [ -z "$file_path" ] && { echo "Error: --file is required" >&2; usage; }
        [ ! -f "$file_path" ] && { echo "Error: File not found: $file_path" >&2; exit 1; }

        # Use curl multipart upload
        echo "Uploading tarball: $file_path"
        curl_args=(
            -X POST
            -H "Authorization: Bearer ${AUTH_TOKEN}"
            -H "Host: ${HOST_HEADER}"
            -sS -w "\n%{http_code}"
            -F "file=@$file_path"
            --resolve "${HOST_HEADER}:443:${PROXY_HOST}"
        )

        upload_url="https://${HOST_HEADER}:443/api/v1/images/upload?image_name=${image_name}&size_mib=${size_mib}"
        response=$(sudo curl "${curl_args[@]}" "${upload_url}")

        http_code=$(echo "$response" | tail -n1)
        body=$(echo "$response" | sed '$d')

        if [ "$http_code" -ge 400 ]; then
            echo "Error: HTTP $http_code" >&2
            echo "$body" | jq -r '.error // .' >&2
            exit 1
        fi

        echo "$body" | jq '.' 2>/dev/null || echo "$body"
        ;;

    image-status)
        [ -z "${2:-}" ] && usage
        image_name="$2"
        shift 2
        poll=false
        while [ $# -gt 0 ]; do
            case "$1" in
                --poll) poll=true; shift ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        if [ "$poll" = true ]; then
            echo "Polling for image status (Ctrl+C to cancel)..."
            while true; do
                response=$(api_call GET "/images/$image_name/status" 2>&1) || {
                    echo "Error fetching status: $response" >&2
                    exit 1
                }
                status=$(echo "$response" | jq -r '.status // "unknown"')
                echo "Status: $status"

                case "$status" in
                    completed)
                        echo "Image creation completed!"
                        echo "$response" | jq '.'
                        exit 0
                        ;;
                    failed)
                        echo "Image creation failed!"
                        echo "$response" | jq '.'
                        exit 1
                        ;;
                    *)
                        sleep 5
                        ;;
                esac
            done
        else
            api_call GET "/images/$image_name/status"
        fi
        ;;

    image-delete)
        [ -z "${2:-}" ] && usage
        image_id="$2"
        api_call DELETE "/images/$image_id"
        ;;

    # ====== Tag Management Commands ======

    tag-create)
        shift
        tag_name=""
        commit_id=""
        description=""

        while [ $# -gt 0 ]; do
            case "$1" in
                --name) tag_name="$2"; shift 2 ;;
                --commit) commit_id="$2"; shift 2 ;;
                --description) description="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ -z "$tag_name" ] && { echo "Error: --name is required" >&2; usage; }
        [ -z "$commit_id" ] && { echo "Error: --commit is required" >&2; usage; }

        # Build the request JSON
        request="{\"tag_name\":\"$tag_name\",\"commit_id\":\"$commit_id\""
        if [ -n "$description" ]; then
            request="$request,\"description\":\"$description\""
        fi
        request="$request}"

        api_call POST "/commit_tags" "$request"
        ;;

    tag-list)
        api_call GET "/commit_tags"
        ;;

    tag-get)
        [ -z "${2:-}" ] && usage
        tag_name="$2"
        api_call GET "/commit_tags/$tag_name"
        ;;

    tag-update)
        [ -z "${2:-}" ] && usage
        tag_name="$2"
        shift 2
        commit_id=""
        description=""
        has_updates=false

        while [ $# -gt 0 ]; do
            case "$1" in
                --commit) commit_id="$2"; has_updates=true; shift 2 ;;
                --description) description="$2"; has_updates=true; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ "$has_updates" = false ] && { echo "Error: At least one of --commit or --description is required" >&2; usage; }

        # Build the request JSON
        request="{"
        parts=()
        [ -n "$commit_id" ] && parts+=("\"commit_id\":\"$commit_id\"")
        [ -n "$description" ] && parts+=("\"description\":\"$description\"")

        IFS=,
        request+="${parts[*]}"
        unset IFS
        request+="}"

        api_call PATCH "/commit_tags/$tag_name" "$request"
        ;;

    tag-delete)
        [ -z "${2:-}" ] && usage
        tag_name="$2"
        api_call DELETE "/commit_tags/$tag_name"
        ;;

    # ====== Domain Management Commands ======

    domain-create)
        shift
        vm_id=""
        domain=""

        while [ $# -gt 0 ]; do
            case "$1" in
                --vm) vm_id="$2"; shift 2 ;;
                --domain) domain="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ -z "$vm_id" ] && { echo "Error: --vm is required" >&2; usage; }
        [ -z "$domain" ] && { echo "Error: --domain is required" >&2; usage; }

        api_call POST "/domains" "{\"vm_id\":\"$vm_id\",\"domain\":\"$domain\"}"
        ;;

    domain-list)
        shift
        query_params=""

        while [ $# -gt 0 ]; do
            case "$1" in
                --vm)
                    if [ -z "$query_params" ]; then
                        query_params="?vm_id=$2"
                    else
                        query_params="${query_params}&vm_id=$2"
                    fi
                    shift 2
                    ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        api_call GET "/domains${query_params}"
        ;;

    domain-get)
        [ -z "${2:-}" ] && usage
        domain_id="$2"
        api_call GET "/domains/$domain_id"
        ;;

    domain-delete)
        [ -z "${2:-}" ] && usage
        domain_id="$2"
        api_call DELETE "/domains/$domain_id"
        ;;

    # ====== Admin Commands ======

    generate-api-key)
        shift
        user_id="$DEFAULT_USER_ID"
        org_id="$DEFAULT_ORG_ID"
        label="generated"

        while [ $# -gt 0 ]; do
            case "$1" in
                --user) user_id="$2"; shift 2 ;;
                --org) org_id="$2"; shift 2 ;;
                --label) label="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        [ -z "$user_id" ] && { echo "Error: --user is required" >&2; usage; }
        [ -z "$org_id" ] && { echo "Error: --org is required" >&2; usage; }

        admin_api_call POST "/admin/api_key" "{\"user_id\":\"$user_id\",\"org_id\":\"$org_id\",\"label\":\"$label\"}" | jq -r '.api_key'
        ;;

    sleep)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        skip_wait_boot="false"
        while [ $# -gt 0 ]; do
            case "$1" in
                --skip-wait-boot) skip_wait_boot="true"; shift ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        admin_api_call POST "/admin/vm/$vm_id/sleep" "{\"skip_wait_boot\":$skip_wait_boot}"
        ;;

    wake)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        node_id=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --node) node_id="$2"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        if [ -n "$node_id" ]; then
            admin_api_call POST "/admin/vm/$vm_id/wake" "{\"destination_node_id\":\"$node_id\"}"
        else
            admin_api_call POST "/admin/vm/$vm_id/wake" "{}"
        fi
        ;;

    move)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        node_id=""
        skip_wait_boot="false"
        while [ $# -gt 0 ]; do
            case "$1" in
                --node) node_id="$2"; shift 2 ;;
                --skip-wait-boot) skip_wait_boot="true"; shift ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done
        if [ -n "$node_id" ]; then
            admin_api_call POST "/admin/vm/$vm_id/move" "{\"destination_node_id\":\"$node_id\",\"skip_wait_boot\":$skip_wait_boot}"
        else
            admin_api_call POST "/admin/vm/$vm_id/move" "{\"skip_wait_boot\":$skip_wait_boot}"
        fi
        ;;

    label)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        shift 2
        labels=()

        while [ $# -gt 0 ]; do
            case "$1" in
                --label) labels+=("$2"); shift 2 ;;
                *) echo "Unknown option: $1" >&2; usage ;;
            esac
        done

        if [ ${#labels[@]} -eq 0 ]; then
            echo "Error: at least one --label is required" >&2
            usage
        fi

        labels_json=$(printf '%s\n' "${labels[@]}" | jq -R 'split("=") | {(.[0]): .[1]}' | jq -s 'add')
        api_call PATCH "/vm/$vm_id/label" "{\"labels\":$labels_json}"
        ;;

    # ====== Proxy TLS Testing Commands ======
    # These commands test the proxy's TLS functionality by sending requests
    # to VMs through the proxy using either UUID subdomains or custom domains

    vm-request-uuid)
        [ -z "${2:-}" ] && usage
        vm_id="$2"
        path="${3:-/}"

        # Ensure path starts with /
        [[ "$path" != /* ]] && path="/$path"

        vm_domain="${vm_id}.${VM_BASE_DOMAIN}"

        echo "Sending HTTPS request to VM through proxy:" >&2
        echo "  VM Domain: $vm_domain" >&2
        echo "  Path: $path" >&2
        echo "  Proxy: ${PROXY_HOST}:${PROXY_PORT}" >&2
        echo "" >&2

        # Set Host header explicitly without port to avoid hostname validation errors
        response=$(curl -sS -v -w "\n---HTTP_CODE:%{http_code}---" \
            -H "Host: ${vm_domain}" \
            --resolve "${vm_domain}:${PROXY_PORT}:${PROXY_HOST}" \
            "https://${vm_domain}:${PROXY_PORT}${path}" 2>&1)

        http_code=$(echo "$response" | grep -o "HTTP_CODE:[0-9]*" | cut -d: -f2)

        echo "$response" | sed 's/---HTTP_CODE:[0-9]*---//'

        # 500 errors mean proxy routed correctly, VM just isn't ready
        if [ -n "$http_code" ] && [ "$http_code" = "500" ]; then
            echo "" >&2
            echo "✓ Proxy routing successful (VM returned 500 - VM may still be booting)" >&2
            exit 0
        elif [ -n "$http_code" ] && [ "$http_code" -ge 200 ] && [ "$http_code" -lt 600 ]; then
            echo "" >&2
            echo "✓ Proxy routing successful (HTTP $http_code)" >&2
            exit 0
        fi
        ;;

    vm-request-custom)
        [ -z "${2:-}" ] && usage
        custom_domain="$2"
        path="${3:-/}"

        # Ensure path starts with /
        [[ "$path" != /* ]] && path="/$path"

        echo "Sending HTTPS request to VM through proxy:" >&2
        echo "  Custom Domain: $custom_domain" >&2
        echo "  Path: $path" >&2
        echo "  Proxy: ${PROXY_HOST}:${PROXY_PORT}" >&2
        echo "" >&2

        # Set Host header explicitly without port to avoid hostname validation errors
        # -k flag is set only here, this is because the certs from ACME is self-signed
        # which is expected and okay.
        response=$(curl -k -sS -v -w "\n---HTTP_CODE:%{http_code}---" \
            -H "Host: ${custom_domain}" \
            --resolve "${custom_domain}:${PROXY_PORT}:${PROXY_HOST}" \
            "https://${custom_domain}:${PROXY_PORT}${path}" 2>&1)

        http_code=$(echo "$response" | grep -o "HTTP_CODE:[0-9]*" | cut -d: -f2)

        echo "$response" | sed 's/---HTTP_CODE:[0-9]*---//'

        # 500 errors mean proxy routed correctly, VM just isn't ready
        if [ -n "$http_code" ] && [ "$http_code" = "500" ]; then
            echo "" >&2
            echo "✓ Proxy routing successful (VM returned 500 - VM may still be booting)" >&2
            exit 0
        elif [ -n "$http_code" ] && [ "$http_code" -ge 200 ] && [ "$http_code" -lt 600 ]; then
            echo "" >&2
            echo "✓ Proxy routing successful (HTTP $http_code)" >&2
            exit 0
        fi
        ;;

    acme-challenge)
        if [ -z "${2:-}" ] || [ -z "${3:-}" ]; then
            usage
        fi
        domain="$2"
        token="$3"

        challenge_path="/.well-known/acme-challenge/${token}"

        echo "Testing ACME HTTP-01 challenge endpoint:" >&2
        echo "  Domain: $domain" >&2
        echo "  Token: $token" >&2
        echo "  Path: $challenge_path" >&2
        echo "  Proxy: ${PROXY_HOST}:${PROXY_PORT}" >&2
        echo "" >&2

        # Set Host header explicitly without port to avoid hostname validation errors
        curl -k -sS -v \
            -H "Host: ${domain}" \
            --resolve "${domain}:${PROXY_PORT}:${PROXY_HOST}" \
            "https://${domain}:${PROXY_PORT}${challenge_path}" 2>&1
        ;;

    *)
        usage
        ;;
esac
