#!/bin/bash

set -eoux pipefail

cd "$(dirname "$0")/.."

POSTGRES_PASSWORD=$PGPASSWORD
POSTGRES_USER=postgres
POSTGRES_DB=vers
POSTGRES_HOST=vers.cwxoqiosmfyv.us-east-1.rds.amazonaws.com
PG=postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@${POSTGRES_HOST}:5432/${POSTGRES_DB}

dbmate --url $PG \
       --migrations-dir ./migrations \
       --no-dump-schema \
       up \
       --strict
