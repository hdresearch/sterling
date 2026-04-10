#!/bin/bash

# Enhanced Chelsea API Test Script with Authentication Testing
# This script tests API functionality including error handling and authentication

# Function to display usage information
show_usage() {
  echo "Usage: $0 <api_url> <valid_api_key> <no_permission_key>"
  echo "Example: $0 http://localhost:8080 valid_key_123 another_valid_key_456"
  echo "  <api_url>       - Base URL for the Chelsea API"
  echo "  <valid_api_key> - A valid API key"
  echo "  <no_permission_key> - Another valid API key different from the first"
  exit 1
}

# Check if all three arguments were provided
if [ $# -lt 3 ]; then
  show_usage
fi

# Set the base URL for the API from command line parameter
API_URL="$1"

# Set API keys based on command line args
VALID_API_KEY="$2"
NO_PERMISSION_KEY="$3"
INVALID_API_KEY="this_key_doesnt_exist"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Generate random IDs for testing
NON_EXISTENT_CLUSTER_ID="nonexistent123"
NON_EXISTENT_VM_ID="nonexistent456"

# Global variables to store IDs of created resources
CLUSTER_ID=""
ROOT_VM_ID=""
CHILD_VM_ID=""
GRANDCHILD_VM_ID=""

# Function to print colored output
print_msg() {
  local color=$1
  local message=$2
  echo -e "${color}${message}${NC}"
}

# Function to test an API endpoint and verify the response
test_endpoint() {
  local description=$1
  local command=$2
  local expected_status=$3
  local expected_content=$4
  
  echo -e "\n----- Testing: $description -----"
  echo "Command: $command"
  
  # Run the command and capture status code and response
  response=$(eval $command -w "%{http_code}" -s)
  status_code=${response: -3}
  body=${response:0:${#response}-3}
  
  # Check status code
  if [[ "$status_code" == "$expected_status" ]]; then
    echo -e "${GREEN}✓ Status code: $status_code [CORRECT]${NC}"
  else
    echo -e "${RED}✗ Status code: $status_code (expected $expected_status) [FAILED]${NC}"
  fi
  
  # Check response content
  if [[ -z "$expected_content" ]] || [[ "$body" == *"$expected_content"* ]]; then
    echo -e "${GREEN}✓ Response contains expected content [CORRECT]${NC}"
  else
    echo -e "${RED}✗ Response does not contain: '$expected_content' [FAILED]${NC}"
    echo "Actual response: $body"
  fi
  
  # Return the body for further processing if needed
  echo "$body"
}

# Function to test standard authenticated endpoint (needs a valid API key)
test_auth_endpoint() {
  local operation=$1
  local endpoint=$2
  local method=$3
  local data=$4
  local success_content=$5
  
  print_msg $PURPLE "\n=== Authentication Testing for: $operation ==="
  
  # Test with no auth
  test_endpoint \
    "$operation with NO AUTH" \
    "curl -X $method $API_URL$endpoint \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "401" \
    "API key is required"
  
  # Test with invalid auth
  test_endpoint \
    "$operation with INVALID AUTH" \
    "curl -X $method $API_URL$endpoint \
    -H 'Authorization: Bearer $INVALID_API_KEY' \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "401" \
    "Invalid API key"
  
  # Test with valid auth (actual operation)
  response=$(test_endpoint \
    "$operation with VALID AUTH" \
    "curl -X $method $API_URL$endpoint \
    -H 'Authorization: Bearer $VALID_API_KEY' \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "200" \
    "$success_content")
  
  # Return the response for further processing
  echo "$response"
}

# Function to test resource-specific endpoint (needs a valid API key with permission)
test_resource_auth_endpoint() {
  local operation=$1
  local endpoint=$2
  local method=$3
  local data=$4
  local success_content=$5
  
  print_msg $PURPLE "\n=== Resource Auth Testing for: $operation ==="
  
  # Test with no auth
  test_endpoint \
    "$operation with NO AUTH" \
    "curl -X $method $API_URL$endpoint \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "401" \
    "API key is required"
  
  # Test with invalid auth - now expect 401, not 403 (invalid key vs. no permission)
  test_endpoint \
    "$operation with INVALID AUTH" \
    "curl -X $method $API_URL$endpoint \
    -H 'Authorization: Bearer $INVALID_API_KEY' \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "401" \
    "Invalid API key"
  
  # Test with valid auth but no permission - expect 403 with error in ServiceResponseError format
  test_endpoint \
    "$operation with NO PERMISSION" \
    "curl -X $method $API_URL$endpoint \
    -H 'Authorization: Bearer $NO_PERMISSION_KEY' \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "403" \
    "\"error\""
  
  # Test with valid auth with permission
  response=$(test_endpoint \
    "$operation with VALID PERMISSION" \
    "curl -X $method $API_URL$endpoint \
    -H 'Authorization: Bearer $VALID_API_KEY' \
    ${data:+-H 'Content-Type: application/json' -d '$data'}" \
    "200" \
    "$success_content")
  
  # Return the response for further processing
  echo "$response"
}

echo "====================================================="
print_msg $BLUE "Chelsea API Comprehensive Test Suite"
echo "====================================================="
print_msg $GREEN "Using API URL: $API_URL"
print_msg $GREEN "Valid API Key: $VALID_API_KEY"
print_msg $GREEN "Invalid API Key: $INVALID_API_KEY"
print_msg $GREEN "No Permission Key: $NO_PERMISSION_KEY"

# Step 1: Authentication Testing for Cluster Creation
print_msg $YELLOW "\nStep 1: Creating a new cluster with authentication testing"

# Test auth for cluster creation - any valid API key can create a cluster
print_msg $PURPLE "\n=== Authentication Testing for: Create Cluster ==="

# Test with no auth
test_endpoint \
  "Create Cluster with NO AUTH" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": 1024, \"vcpu_count\": 1}'" \
  "401" \
  "API key is required"

# Test with invalid auth
test_endpoint \
  "Create Cluster with INVALID AUTH" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Authorization: Bearer $INVALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": 1024, \"vcpu_count\": 1}'" \
  "401" \
  "Invalid API key"

# Test with valid API key (this should work)
response=$(test_endpoint \
  "Create Cluster with VALID API KEY" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": 1024, \"vcpu_count\": 1}'" \
  "201" \
  "data")

# Extract cluster ID and root VM ID - adjusted for ServiceResponseSuccess format
CLUSTER_ID=$(echo "$response" | grep -o '\"id\":\"[^\"]*\"' | head -1 | cut -d'"' -f4 | tr -d '[:space:]')
ROOT_VM_ID=$(echo "$response" | grep -o '\"root_vm_id\":\"[^\"]*\"' | head -1 | cut -d'"' -f4 | tr -d '[:space:]')

# Validate extracted IDs
echo "Extracted CLUSTER_ID='$CLUSTER_ID'"
echo "Extracted ROOT_VM_ID='$ROOT_VM_ID'"

if [[ -z "$CLUSTER_ID" || -z "$ROOT_VM_ID" ]]; then
  print_msg $RED "Failed to extract cluster or root VM ID. Exiting."
  exit 1
fi

print_msg $GREEN "Created cluster with ID: $CLUSTER_ID"
print_msg $GREEN "Root VM ID: $ROOT_VM_ID"

# Step 2: Authentication Testing for Branching VM
print_msg $YELLOW "\nStep 2: Creating a child VM by branching the root VM with auth testing"

print_msg $YELLOW "(sleeping for 2.0 seconds to ensure root is finished booting)"
sleep 2.0

# Test auth for branching VM - requires permission for the specific VM
response=$(test_resource_auth_endpoint \
  "Branch VM" \
  "/api/vm/$ROOT_VM_ID/branch" \
  "POST" \
  '{}' \
  "data")

# Extract child VM ID - adjusted for ServiceResponseSuccess format
CHILD_VM_ID=$(echo "$response" | grep -o '\"id\":\"[^\"]*\"' | head -1 | cut -d'"' -f4 | tr -d '[:space:]')
echo "Extracted CHILD_VM_ID='$CHILD_VM_ID'"

if [[ -z "$CHILD_VM_ID" ]]; then
  print_msg $RED "Failed to create child VM. Exiting."
  exit 1
fi

print_msg $GREEN "Created child VM with ID: $CHILD_VM_ID"

# Step 3: Authentication Testing for Branching Child VM
print_msg $YELLOW "\nStep 3: Creating a grandchild VM by branching the child VM with auth testing"

# Test auth for branching child VM - requires permission for the specific VM
response=$(test_resource_auth_endpoint \
  "Branch Child VM" \
  "/api/vm/$CHILD_VM_ID/branch" \
  "POST" \
  '{}' \
  "data")

# Extract grandchild VM ID - adjusted for ServiceResponseSuccess format
GRANDCHILD_VM_ID=$(echo "$response" | grep -o '\"id\":\"[^\"]*\"' | head -1 | cut -d'"' -f4 | tr -d '[:space:]')
echo "Extracted GRANDCHILD_VM_ID='$GRANDCHILD_VM_ID'"

if [[ -z "$GRANDCHILD_VM_ID" ]]; then
  print_msg $RED "Failed to create grandchild VM. Exiting."
  exit 1
fi

print_msg $GREEN "Created grandchild VM with ID: $GRANDCHILD_VM_ID"

print_msg $BLUE "\n====================================================="
print_msg $BLUE "Starting Error Tests with Authentication"
print_msg $BLUE "====================================================="

# Test Set 1: GET operations with auth testing
print_msg $CYAN "\nTesting GET operations with authentication"

# Get non-existent cluster - expect 403 with ServiceResponseError format
test_endpoint \
  "Get Non-existent Cluster" \
  "curl -X GET $API_URL/api/cluster/$NON_EXISTENT_CLUSTER_ID \
  -H 'Authorization: Bearer $VALID_API_KEY'" \
  "403" \
  "\"error\""

# Get non-existent VM - expect 403 with ServiceResponseError format
test_endpoint \
  "Get Non-existent VM" \
  "curl -X GET $API_URL/api/vm/$NON_EXISTENT_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY'" \
  "403" \
  "\"error\""

# Get existing cluster with auth testing (requires permission)
test_resource_auth_endpoint \
  "Get Existing Cluster" \
  "/api/cluster/$CLUSTER_ID" \
  "GET" \
  "" \
  "data"

# Get existing VM with auth testing (requires permission)
test_resource_auth_endpoint \
  "Get Existing VM" \
  "/api/vm/$ROOT_VM_ID" \
  "GET" \
  "" \
  "state"

# Test Set 2: Invalid operations with auth testing
print_msg $CYAN "\nTesting Invalid Operations with authentication"

# Invalid cluster creation (negative memory) - any valid API key 
print_msg $PURPLE "\n=== Authentication Testing for: Invalid Cluster Creation ==="

# Test with no auth
test_endpoint \
  "Invalid Cluster Creation with NO AUTH" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": -1, \"vcpu_count\": 1}'" \
  "422" \
  "Failed to deserialize"

# Test with invalid auth
test_endpoint \
  "Invalid Cluster Creation with INVALID AUTH" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Authorization: Bearer $INVALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": -1, \"vcpu_count\": 1}'" \
  "422" \
  "Failed to deserialize"

# Test with valid auth (this should fail with 422 for invalid input)
test_endpoint \
  "Invalid Cluster Creation with VALID AUTH" \
  "curl -X POST $API_URL/api/cluster \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"mem_size_mib\": -1, \"vcpu_count\": 1}'" \
  "422" \
  "Failed to deserialize"

# Try to resume root VM which is paused and has children - requires permission
# Using PATCH for state updates - expect 409 with ServiceResponseError format
test_endpoint \
  "Resume VM with Children" \
  "curl -X PATCH $API_URL/api/vm/$ROOT_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"resume\"}'" \
  "409" \
  "\"error\""

# Try to branch a non-existent VM - expect 403 with ServiceResponseError format
test_endpoint \
  "Branch Non-existent VM" \
  "curl -X POST $API_URL/api/vm/$NON_EXISTENT_VM_ID/branch \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{}'" \
  "403" \
  "\"error\""

# Test Set 3: Additional VM state operations with auth testing
print_msg $CYAN "\nTesting Additional VM State Operations with authentication"

# Pause an already running VM (grandchild) - using PATCH for VM state operations
test_endpoint \
  "Pause Running VM" \
  "curl -X PATCH $API_URL/api/vm/$GRANDCHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"pause\"}'" \
  "200" \
  "Paused"

# Resume the grandchild VM again
test_endpoint \
  "Resume Paused VM" \
  "curl -X PATCH $API_URL/api/vm/$GRANDCHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"resume\"}'" \
  "200" \
  "Running"

# Try to resume an already running VM - expect 409 with ServiceResponseError format
test_endpoint \
  "Resume Already Running VM" \
  "curl -X PATCH $API_URL/api/vm/$GRANDCHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"resume\"}'" \
  "409" \
  "\"error\""

# Pause VM
test_endpoint \
  "Pause VM" \
  "curl -X PATCH $API_URL/api/vm/$GRANDCHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"pause\"}'" \
  "200" \
  "Paused"

# Pause an already paused VM (after pausing it) - expect 409 with ServiceResponseError format
test_endpoint \
  "Pause Already Paused VM" \
  "curl -X PATCH $API_URL/api/vm/$GRANDCHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY' \
  -H 'Content-Type: application/json' -d '{\"action\": \"pause\"}'" \
  "409" \
  "\"error\""

# Test list endpoints with authentication
print_msg $CYAN "\nTesting List Operations with authentication"

# Test listing all clusters (filtered by API key)
test_auth_endpoint \
  "List All Clusters" \
  "/api/cluster" \
  "GET" \
  "" \
  "data"

# Test listing all VMs (filtered by API key)
test_auth_endpoint \
  "List All VMs" \
  "/api/vm" \
  "GET" \
  "" \
  "data"

# Test Set 4: DELETE operations with auth testing
print_msg $CYAN "\nTesting DELETE operations with authentication"

# Try to delete a root VM with auth testing - requires permission, expect 400 with ServiceResponseError
test_endpoint \
  "Delete Root VM" \
  "curl -X DELETE $API_URL/api/vm/$ROOT_VM_ID?recursive=true \
  -H 'Authorization: Bearer $VALID_API_KEY'" \
  "400" \
  "\"error\""

# Try to delete a VM with children without recursive flag - expect 409 with ServiceResponseError
test_endpoint \
  "Delete VM with Children" \
  "curl -X DELETE $API_URL/api/vm/$CHILD_VM_ID \
  -H 'Authorization: Bearer $VALID_API_KEY'" \
  "409" \
  "\"error\""

# Try to delete a non-existent cluster - expect 403 with ServiceResponseError
test_endpoint \
  "Delete Non-existent Cluster" \
  "curl -X DELETE $API_URL/api/cluster/$NON_EXISTENT_CLUSTER_ID \
  -H 'Authorization: Bearer $VALID_API_KEY'" \
  "403" \
  "\"error\""

# Cleanup: Delete the VMs and cluster with auth
print_msg $BLUE "\n====================================================="
print_msg $BLUE "Cleaning up resources with authentication"
print_msg $BLUE "====================================================="

# Delete grandchild VM with auth (requires permission)
print_msg $YELLOW "Deleting grandchild VM: $GRANDCHILD_VM_ID"
curl -X DELETE "$API_URL/api/vm/$GRANDCHILD_VM_ID" \
  -H "Authorization: Bearer $VALID_API_KEY" \
  -s > /dev/null

# Delete child VM with auth (requires permission)
print_msg $YELLOW "Deleting child VM: $CHILD_VM_ID"
curl -X DELETE "$API_URL/api/vm/$CHILD_VM_ID" \
  -H "Authorization: Bearer $VALID_API_KEY" \
  -s > /dev/null

# Delete cluster with auth (requires permission)
print_msg $YELLOW "Deleting cluster: $CLUSTER_ID"
curl -X DELETE "$API_URL/api/cluster/$CLUSTER_ID" \
  -H "Authorization: Bearer $VALID_API_KEY" \
  -s > /dev/null

print_msg $GREEN "\nAPI Testing Complete: Functionality, Error Handling, and Authentication"
print_msg $GREEN "Tests ran with Valid API Key: $VALID_API_KEY"
print_msg $GREEN "Tests ran with Invalid API Key: $INVALID_API_KEY"
print_msg $GREEN "Tests ran with No Permission Key: $NO_PERMISSION_KEY"
