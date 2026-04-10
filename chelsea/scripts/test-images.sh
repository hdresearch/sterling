#!/bin/bash
#
# test-images.sh - E2E tests for base image API against single-node deployment
#
# Usage:
#   ./scripts/test-images.sh          # Run all tests (assumes single-node is running)
#   ./scripts/test-images.sh --start  # Start single-node first, then run tests
#

set -e

BASE_URL="http://[fd00:fe11:deed::1]:8111/api"
DOCKER_IMAGE="chelsea-ubuntu-24.04"
TEST_PREFIX="test-img-$$"
PASSED=0
FAILED=0
TESTS_RUN=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $1"
    ((PASSED++))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    ((FAILED++))
}

# Check if single-node is running
check_chelsea_running() {
    if curl -s --connect-timeout 5 "$BASE_URL/../health" > /dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

# Wait for image creation to complete
wait_for_image() {
    local image_name="$1"
    local timeout="${2:-60}"
    local start=$(date +%s)

    while true; do
        local elapsed=$(($(date +%s) - start))
        if [ $elapsed -gt $timeout ]; then
            echo "timeout"
            return 1
        fi

        local status_json=$(curl -s "$BASE_URL/images/$image_name/status" 2>/dev/null)
        local status=$(echo "$status_json" | jq -r '.status // empty' 2>/dev/null)

        case "$status" in
            completed)
                echo "completed"
                return 0
                ;;
            failed)
                echo "failed"
                return 1
                ;;
            *)
                sleep 1
                ;;
        esac
    done
}

# Cleanup test images
cleanup_test_images() {
    log_info "Cleaning up test images..."
    local images=$(curl -s "$BASE_URL/images" 2>/dev/null | jq -r '.images[].image_name // empty' 2>/dev/null)
    for img in $images; do
        if [[ "$img" == ${TEST_PREFIX}* ]]; then
            curl -s -X DELETE "$BASE_URL/images/$img" > /dev/null 2>&1 || true
            log_info "  Deleted: $img"
        fi
    done
}

# Test: List images (should include default)
test_list_images() {
    ((TESTS_RUN++))
    log_info "Test: List images includes default"

    local result=$(curl -s "$BASE_URL/images")
    local has_default=$(echo "$result" | jq -r '.images[] | select(.image_name == "default") | .image_name' 2>/dev/null)

    if [ "$has_default" == "default" ]; then
        log_pass "Default image found in list"
        return 0
    else
        log_fail "Default image not found. Response: $result"
        return 1
    fi
}

# Test: Create image from Docker
test_create_image() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-create"
    log_info "Test: Create image from Docker ($image_name)"

    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_IMAGE\"},\"size_mib\":512}"
    local result=$(curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload")

    local status=$(echo "$result" | jq -r '.status // empty' 2>/dev/null)
    if [ "$status" != "pending" ] && [ "$status" != "completed" ]; then
        log_fail "Create returned unexpected status: $result"
        return 1
    fi

    # Wait for completion
    log_info "  Waiting for image creation..."
    local final_status=$(wait_for_image "$image_name" 120)

    if [ "$final_status" == "completed" ]; then
        log_pass "Image created successfully"
        return 0
    else
        log_fail "Image creation failed or timed out: $final_status"
        return 1
    fi
}

# Test: Create image with custom size
test_create_image_custom_size() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-sized"
    local custom_size=1024
    log_info "Test: Create image with custom size ($image_name, ${custom_size}MiB)"

    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_IMAGE\"},\"size_mib\":$custom_size}"
    local result=$(curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload")

    # Wait for completion
    log_info "  Waiting for image creation..."
    local final_status=$(wait_for_image "$image_name" 120)

    if [ "$final_status" != "completed" ]; then
        log_fail "Image creation failed: $final_status"
        return 1
    fi

    # Verify size
    local status_json=$(curl -s "$BASE_URL/images/$image_name/status")
    local reported_size=$(echo "$status_json" | jq -r '.size_mib // 0' 2>/dev/null)

    if [ "$reported_size" == "$custom_size" ]; then
        log_pass "Image created with correct size: ${reported_size}MiB"
        return 0
    else
        log_fail "Size mismatch: expected $custom_size, got $reported_size"
        return 1
    fi
}

# Test: Verify snapshot name
test_verify_snapshot() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-snapshot"
    log_info "Test: Verify created image has correct snapshot ($image_name)"

    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_IMAGE\"},\"size_mib\":512}"
    curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload" > /dev/null

    log_info "  Waiting for image creation..."
    local final_status=$(wait_for_image "$image_name" 120)

    if [ "$final_status" != "completed" ]; then
        log_fail "Image creation failed: $final_status"
        return 1
    fi

    # Get the image from list and verify snapshot name
    local list_json=$(curl -s "$BASE_URL/images")
    local snapshot_name=$(echo "$list_json" | jq -r ".images[] | select(.image_name == \"$image_name\") | .snapshot_name // empty" 2>/dev/null)

    if [ "$snapshot_name" == "chelsea_base_image" ]; then
        log_pass "Snapshot name is correct: $snapshot_name"
        return 0
    else
        log_fail "Wrong snapshot name: expected 'chelsea_base_image', got '$snapshot_name'"
        return 1
    fi
}

# Test: Delete image
# NOTE: Delete endpoint is not yet implemented in chelsea_server2
test_delete_image() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-delete"
    log_info "Test: Delete image ($image_name) [SKIPPED - endpoint not implemented]"

    # Create image first (so cleanup will work)
    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_IMAGE\"},\"size_mib\":512}"
    curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload" > /dev/null

    log_info "  Waiting for image creation..."
    local final_status=$(wait_for_image "$image_name" 120)

    if [ "$final_status" != "completed" ]; then
        log_fail "Setup failed - couldn't create image: $final_status"
        return 1
    fi

    # Delete endpoint returns 404 (not implemented)
    local http_code=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$BASE_URL/images/$image_name")

    if [ "$http_code" == "404" ]; then
        log_pass "Delete endpoint correctly returns 404 (not yet implemented)"
        return 0
    else
        log_fail "Unexpected HTTP code for delete: $http_code"
        return 1
    fi
}

# Test: Duplicate name rejected
test_duplicate_name_rejected() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-dup"
    log_info "Test: Duplicate image name rejected ($image_name)"

    # Create first image
    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_IMAGE\"},\"size_mib\":512}"
    curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload" > /dev/null

    log_info "  Waiting for first image creation..."
    local final_status=$(wait_for_image "$image_name" 120)

    if [ "$final_status" != "completed" ]; then
        log_fail "Setup failed - couldn't create first image: $final_status"
        return 1
    fi

    # Try to create second image with same name - should get 409 Conflict
    local http_code=$(curl -s -o /tmp/dup_result.txt -w "%{http_code}" -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload")

    local response=$(cat /tmp/dup_result.txt)

    if [ "$http_code" == "409" ]; then
        log_pass "Duplicate name correctly rejected with HTTP 409: $response"
        return 0
    else
        log_fail "Expected HTTP 409, got $http_code: $response"
        return 1
    fi
}

# Test: Invalid Docker image fails
test_invalid_docker_image() {
    ((TESTS_RUN++))
    local image_name="${TEST_PREFIX}-invalid"
    log_info "Test: Invalid Docker image fails gracefully ($image_name)"

    local payload="{\"image_name\":\"$image_name\",\"source\":{\"type\":\"docker\",\"image_ref\":\"nonexistent-image-12345\"},\"size_mib\":512}"
    curl -s -X POST "$BASE_URL/images/create" \
        -H "Content-Type: application/json" \
        -d "$payload" > /dev/null

    # Wait a bit and check status
    sleep 3

    local status_json=$(curl -s "$BASE_URL/images/$image_name/status" 2>/dev/null)
    local status=$(echo "$status_json" | jq -r '.status // empty' 2>/dev/null)

    if [ "$status" == "failed" ] || [ -z "$status" ]; then
        log_pass "Invalid Docker image correctly failed"
        return 0
    else
        log_fail "Expected failure status, got: $status"
        return 1
    fi
}

# Main
main() {
    echo ""
    echo "========================================"
    echo "  Base Image API E2E Tests"
    echo "========================================"
    echo ""

    # Handle --start flag
    if [ "$1" == "--start" ]; then
        log_info "Starting single-node deployment..."
        ./scripts/single-node.sh start
        sleep 5
    fi

    # Check if Chelsea is running
    if ! check_chelsea_running; then
        log_fail "Chelsea is not running. Start with: ./scripts/single-node.sh start"
        exit 1
    fi

    log_info "Chelsea is running, starting tests..."
    echo ""

    # Cleanup any leftover test images from previous runs
    cleanup_test_images
    echo ""

    # Run tests
    test_list_images || true
    echo ""

    test_create_image || true
    echo ""

    test_create_image_custom_size || true
    echo ""

    test_verify_snapshot || true
    echo ""

    test_delete_image || true
    echo ""

    test_duplicate_name_rejected || true
    echo ""

    test_invalid_docker_image || true
    echo ""

    # Cleanup
    cleanup_test_images

    # Summary
    echo ""
    echo "========================================"
    echo "  Test Summary"
    echo "========================================"
    echo "  Total:  $TESTS_RUN"
    echo -e "  ${GREEN}Passed: $PASSED${NC}"
    echo -e "  ${RED}Failed: $FAILED${NC}"
    echo "========================================"
    echo ""

    if [ $FAILED -gt 0 ]; then
        exit 1
    fi
    exit 0
}

main "$@"
