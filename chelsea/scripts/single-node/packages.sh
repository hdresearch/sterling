#!/bin/bash

# Grab all package names from the package data file.
package_names_from_file() {
  [ -r "$1" ] || { printf 'cannot read %s\n' "$1" >&2; return 1; }

  awk '
    /^[[:space:]]*(#|$)/ { next }         # skip blank / comments lines
    { sub(/[[:space:]]+#.*/, "", $0) }    # strip inline comments
    { for (i = 1; i <= NF; i++) print $i }
  ' "$1" | sort -u
}

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                              Install Packages                              │
# └────────────────────────────────────────────────────────────────────────────┘
apt-get update

scriptdir=`dirname "$BASH_SOURCE"`
package_names_from_file "${scriptdir}/packages.txt" | xargs -r apt-get install -y
