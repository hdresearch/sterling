#!/bin/sh

API_KEY=ef90fd52-66b5-47e7-b7dc-e73c4381028fbfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c

if [ -z "$1" ]; then
    echo "You must specify staging (next) or production"
    exit 1
fi

if [ "$1" != "staging" ] && [ "$1" != "production" ]; then
    echo "You must specify staging or production"
    exit 1
fi

if [ "$1" = "staging" ]; then
    endpoint=https://api.staging.vers.sh
else
    endpoint=https://api.vers.sh
fi

curl \
    -sS \
    --fail \
    -H "Authorization: Bearer $API_KEY" \
    -H 'Accept: application/json' \
    "$endpoint/api/v1/vms" > /dev/null 2>&1

if [ $? -ne 0 ]; then
    echo -e "\e[0;31m⚠\e[0m Cannot list VMs!"
    exit 1
else
    echo -e "\e[0;32m✓\e[0m List   VM ok"
fi



res=$(mktemp)
http_code=$(curl \
    -sS \
    -w '%{http_code}' \
    -H 'Accept: application/json' \
    -H "Authorization: Bearer $API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{"vm_config":{"fs_size_mib":1024,"image_name":"default","kernel_name":"default.bin","mem_size_mib":512,"vcpu_count":1}}' \
    -X POST "$endpoint/api/v1/vm/new_root" \
    -o $res)

if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
    echo -e "\e[0;31m⚠\e[0m Cannot create a VM! (HTTP $http_code)"
    echo "    Response: $(cat $res)"
    rm $res
    exit 1
else
    VM_1=$(jq --raw-output .vm_id $res)
    echo -e "\e[0;32m✓\e[0m Create VM ok $VM_1"
fi
rm $res


res=$(mktemp)
http_code=$(curl \
    -sS \
    -w '%{http_code}' \
    -H 'Accept: application/json' \
    -H "Authorization: Bearer $API_KEY" \
    -X POST "$endpoint/api/v1/vm/$VM_1/branch?count=1" \
    -o $res)

if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
    echo -e "\e[0;31m⚠\e[0m Cannot branch a VM! (HTTP $http_code)"
    echo "    Response: $(cat $res)"
    rm $res
    exit 1
else
    VM_2=$(jq --raw-output .vms[0].vm_id $res)    
    echo -e "\e[0;32m✓\e[0m Branch VM ok $VM_2"
fi
rm $res


res=$(mktemp)
http_code=$(curl \
    -sS \
    -w '%{http_code}' \
    -H 'Accept: application/json' \
    -H "Authorization: Bearer $API_KEY" \
    -X POST "$endpoint/api/v1/vm/$VM_2/commit" \
    -o $res)

if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
    echo -e "\e[0;31m⚠\e[0m Cannot commit a VM! (HTTP $http_code)"
    echo "    Response: $(cat $res)"
    rm $res
    exit 1
else
    COMMIT=$(jq --raw-output .commit_id $res)
    echo -e "\e[0;32m✓\e[0m Commit VM ok $COMMIT"
fi
rm $res


curl \
    -sS \
    --fail \
    -H 'Accept: application/json' \
    -H "Authorization: Bearer $API_KEY" \
    -H 'Content-Type: application/json' \
    -X DELETE "$endpoint/api/v1/vm/$VM_1" > /dev/null

curl \
    -sS \
    --fail \
    -H 'Accept: application/json' \
    -H "Authorization: Bearer $API_KEY" \
    -H 'Content-Type: application/json' \
    -X DELETE "$endpoint/api/v1/vm/$VM_2"  > /dev/null

if [ $? -ne 0 ]; then
    echo -e "\e[0;31m⚠\e[0m Deleting failed!"
    exit 1
else
    echo -e "\e[0;32m✓\e[0m Delete VM ok"
fi


cat <<EOF
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                                                                            │
 │                         Mission Accomplished!                              │
 │                                                                            │
 └────────────────────────────────────────────────────────────────────────────┘
EOF


