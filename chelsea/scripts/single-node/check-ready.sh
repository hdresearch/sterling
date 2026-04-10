#!/bin/bash

# This script ensures all components of the SNE are running as expected.
# This is intended to be used by CI/CD. If an error is detected, the most recent log will be copied to
# /var/lib/chelsea/vers-sne/{executable_name}-{current_time}.log

chelsea_url='[fd00:fe11:deed::1]:8111/api/system/version'
orchestrator_url='[fd00:fe11:deed::ffff]:8090/api/v1/system/version'
proxy_url='127.0.0.1:8080/version'
host_header='Host: api.vers.sh'

postgres_url=postgresql://postgres:opensesame@localhost:5432/vers

sne_logs_dir="/var/lib/vers-sne/logs"

exit_code=0
time_now=$(date +%s)

print_err() {
    local service_name=$1
    local error=$2
    echo -e "\033[31m${service_name}\033[0m: $error"
}

print_warn() {
    local service_name=$1
    local warning=$2
    echo -e "\033[33m${service_name}\033[0m: $warning"
}

print_ok() {
    local service_name=$1
    echo -e "\033[32m${service_name}\033[0m: Ok"
}

# Inner logic for check_vers_component
check_vers_component_version() {
    local expected_name=$1
    local url=$2

    local res
    res=$(curl -sS "$url" -H "$host_header" --connect-timeout 5)
    if [[ $? -ne 0 ]]; then
        print_err "$expected_name" "Failed to get version info from $url"
        return 1
    fi

    local name
    name=$(echo "$res" | jq -r .executable_name)
    if [[ $? -ne 0 ]]; then
        print_err "$expected_name" "Failed to get version info from $url"
        return 1
    fi

    if [[ "$name" != "$expected_name" ]]; then
        print_err "$expected_name" "$url reported executable name '$name'; expected '$expected_name'"
        return 1
    fi

    print_ok "$expected_name"
}

backup_vers_log() {
    local component_name="$1"
    local log_file_path="${sne_logs_dir}/${component_name}.log"
    local lines_to_tail=20
    
    if [[ ! -f $log_file_path ]]; then
        echo -e "\033[33mWarning\033[m: Failed to find expected log file at path $log_file_path"
        return 1
    fi

    backup_file_path="${sne_logs_dir}/${component_name}-${time_now}.log"
    cp "$log_file_path" "$backup_file_path"
    if [[ $? -eq 0 ]]; then
        echo "Copied log to $backup_file_path"
        echo "Last $lines_to_tail lines:"
        tail -n $lines_to_tail $backup_file_path
    else
        echo "Failed to copy log at $log_file_path"
        return 1
    fi
}

# Assert that the version endpoint $2 returns JSON with .executable_name == $1
check_vers_component() {
    local expected_name="$1"
    local url="$2"

    if ! check_vers_component_version "$expected_name" "$url"; then
        backup_vers_log "$expected_name"
        return 1
    fi
}

# Assert that the ceph cluster status is HEALTH_OK
check_ceph() {
    local status
    status=$(ceph --user chelsea health)
    if [[ $? -ne 0 ]]; then
        print_err "ceph" "Failed to get ceph health"
        return 1
    fi

    local health=$(echo "$status" | grep -Po "HEALTH_[A-Z]+")
    case "$health" in
        HEALTH_ERR)
            print_err "ceph" "status is HEALTH_ERR"
            ceph --user chelsea status
            return 1
            ;;
        HEALTH_WARN)
            print_warn "ceph" "status is HEALTH_WARN"
            ceph --user chelsea status
            ;;
        HEALTH_OK)
            print_ok "ceph"
            ;;
        *)
            print_err "ceph" "unexpected status"
            ceph --user chelsea status
    esac
}

# Assert that SELECT 1 on the DB at $1 returns successfully
check_postgres() {
    psql "$postgres_url" -c "SELECT 1;" > /dev/null
    if [[ $? -ne 0 ]]; then
        print_err "postgres" "Failed to SELECT 1 from $postgres_url"
        return 1
    fi

    print_ok "postgres"
}

check_vers_component chelsea "$chelsea_url" || exit_code=1
check_vers_component orchestrator "$orchestrator_url" || exit_code=1
check_vers_component proxy "$proxy_url" || exit_code=1
check_ceph || exit_code=1
check_postgres || exit_code=1

if [[ $exit_code -ne 0 ]]; then
    echo 'SNE verification failed'
else
    echo 'All components of the SNE started successfully'
fi

exit $exit_code
