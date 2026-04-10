#! /usr/bin/env tclsh

set databaseUrl {postgresql://postgres:opensesame@localhost:5432/vers}
set nodeTelemetryEndpoint {/api/system/telemetry}
set nodeServerPort 8111

proc printUsageAndExit {} {
    puts {usage: ./bootstrap-orch.tcl <nodeIp> <instanceId>
    nodeIp: An IPv4 addr, eg: 127.0.0.1
    instanceId: an AWS instance ID, eg: i-0123456789abcdef0}
    exit 1
}

# Ensures that the given IP is a valid IPv4 address
proc validateIp {ip} {
    set ipRegex {^([0-9]{1,3}\.){3}[0-9]{1,3}$}
    if {![regexp $ipRegex $ip]} {
        puts "Error: Invalid IP address: $ip"
        printUsageAndExit
    }

    foreach octet [split $ip "."] {
        if {$octet > 255 || $octet < 0} {
            puts "Error: IP address component out of range: $octet"
            printUsageAndExit
        }
    }
}

# Ensures that the instanceId matches the AWS format
proc validateInstanceId {instanceId} {
    # AWS instance ID should look like "i-0123456789abcdef0" (EC2), typically "i-" followed by 8 or 17 hex digits
    set regex {^i-[0-9a-f]{8,17}$}
    if {![regexp $regex $instanceId]} {
        puts "Error: Invalid instance ID: $instanceId"
        printUsageAndExit
    }
}

# Generates a random IPv6 address
proc generateRandomIpv6 {} {
    set segments {}
    for {set i 0} {$i < 8} {incr i} {
        set segment [format "%04x" [expr {int(rand() * 0x10000)}]]
        lappend segments $segment
    }
    return [join $segments ":"]
}

# Converts a base64-encoded string into a JSON array of bytes.
proc b64ToJsonArray {b64str} {
    set decoded [exec echo $b64str | base64 --decode | od -An -t u1]
    set numbers [split [string trim $decoded] " "]

    set json_list {}
    foreach n $numbers {
        set n_trim [string trim $n]
        if {$n_trim ne ""} {
            lappend json_list $n_trim
        }
    }

    set json_array [join $json_list ","]
    return "\[$json_array\]"
}

# Convenience wrapper for jq -r
proc jq {input query} {
    return [exec echo $input | jq -r $query]
}

# Set variables from argv
set nodeIp [lindex $argv 0]
validateIp $nodeIp

set instanceId [lindex $argv 1]
validateInstanceId $instanceId

puts "IMPORTANT: Please ensure orchestrator, chelsea, and postgres are all running.
puts "This script will do the following:"
puts "- Insert a record into table 'nodes' at the following URL: $databaseUrl (if this is incorrect, please modify the databaseUrl var in this script)"
puts "- The record will populate its telemetry information from 'GET $nodeIp:$nodeServerPort/$nodeTelemetryEndpoint'"
puts "- Generate a cloudconfig booststrap config and write it to /var/lib/chelsea/bootstrap/config.json"

puts "Press enter to continue."
gets stdin

# Get telemetry info from node
set nodeTelemetry [exec curl -sS $nodeIp:$nodeServerPort$nodeTelemetryEndpoint]

# Fetch orchestrator info from DB (expects orch to be running)
set orchestrator [exec psql $databaseUrl -t -c "SELECT (id, wg_public_key, wg_private_key, wg_ipv6, ip) from orchestrators;"]
set orchestrator [split [string trim [string trim $orchestrator] "()"] ,]

set orchestratorId        [string trim [lindex $orchestrator 0]]
set orchestratorPubkey     [string trim [lindex $orchestrator 1]]
set orchestratorPrivkey    [string trim [lindex $orchestrator 2]]
set orchestratorIpv6       [string trim [lindex $orchestrator 3]]
set orchestratorPublicIpV4 [string trim [lindex $orchestrator 4]]

# Compute fields for node DB entry
set instance_id [format "%016x" [expr {int(rand() * (1 << 65))}]]
set under_orchestrator_id $orchestratorId
set wg_ipv6 [generateRandomIpv6]
set wg_private_key [exec wg genkey]
set wg_public_key [exec echo $wg_private_key | wg pubkey]
set cpu_cores_total [jq $nodeTelemetry .cpu.vcpu_count_total]
set memory_mib_total [jq $nodeTelemetry .ram.vm_mib_available]
set disk_size_mib_total [jq $nodeTelemetry .fs.mib_total]
set network_count_total [jq $nodeTelemetry .chelsea.vm_count_max]
set firstBootPath "/chelsea/firstboot/$instance_id"

# Insert node into DB
set insert_query [format "INSERT INTO nodes (ip, instance_id, under_orchestrator_id, wg_ipv6, wg_public_key, wg_private_key, cpu_cores_total, memory_mib_total, disk_size_mib_total, network_count_total) VALUES ('%s', '%s', '%s', '%s', '%s', '%s', %s, %s, %s, %s);" $nodeIp $instanceId $under_orchestrator_id $wg_ipv6 $wg_public_key $wg_private_key $cpu_cores_total $memory_mib_total $disk_size_mib_total $network_count_total]
puts "Executing psql: '$insert_query'"
exec psql $databaseUrl -c $insert_query

# Fetch the node_id from the newly inserted node record
set select_node_id_query [format "SELECT node_id FROM nodes WHERE ip = '%s';" $nodeIp]
set node_id_result [exec psql $databaseUrl -t -c $select_node_id_query]
set node_id [string trim $node_id_result]

# Generate the JSON CloudInitBootstrapObject and output
set cloudinit_json [format {{
  "orchestrator_ipv6": "%s",
  "orchestrator_public_ipv4": "%s",
  "orchestrator_pubkey": %s,
  "node_ipv6": "%s",
  "node_id": "%s",
  "wg": {
    "private": "%s",
    "public": "%s"
  },
  "first_boot_path": "%s"
}} $orchestratorIpv6 $orchestratorPublicIpV4 [b64ToJsonArray $orchestratorPubkey] $wg_ipv6 $node_id $wg_private_key $wg_public_key $firstBootPath]

puts "Generated CloudInitBootstrapObject JSON:"
puts $cloudinit_json

# Write the bootstrap config to /var/lib/chelsea/bootstrap/config.json (hardcoded path)
exec mkdir -p "/var/lib/chelsea/bootstrap"
puts "Ensured /var/lib/chelsea/bootstrap dir exists"

set fdConfig [open "/var/lib/chelsea/bootstrap/config.json" w]
puts $fdConfig $cloudinit_json
puts "Wrote cloudinit JSON to /var/lib/chelsea/bootstrap/config.json"
