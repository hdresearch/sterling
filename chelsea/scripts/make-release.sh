#!/bin/sh

echo "Run the release script on all hosts"

if [ -z "$1" ]; then
    echo "You must specify staging (next) or production"
    exit 1
fi

if [ "$1" != "staging" ] && [ "$1" != "production" ]; then
    echo "You must specify staging or production"
    exit 1
fi

do_release () {
    out=$(mktemp)
    curl -sS \
         --fail \
         --data-binary "@./scripts/release-$2.sh" \
         "http://[fd00:1:1::1]:3232/sh?host=$1" \
         > "$out" &&
        jq . "$out" &&
        jq --exit-status '[ .[].code ] | all(. == 0)' "$out"
}

if [ "$1" = "staging" ]; then
    do_release next staging #prefix match on all next* nodes
else
    # No common prefix, but do them in order
    do_release chelsea production && \
    do_release orchestrator production && \
    do_release proxy production
fi
