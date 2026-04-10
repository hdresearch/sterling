#!/bin/bash

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │       This is a tool to compare your local dev DB to Production            │
# │                                                                            │
# │       You need to set the PROD_POSTGRES_PASSWORD env variable              │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

set -eou pipefail

# ──────────────────────────────────────────────────────────────────────────────
# Assert some things about the environment in which we are running

if ! command -v psql >/dev/null 2>&1
then
    echo "You need a postgres client installed"
    exit 1
fi

if ! command -v pg-schema-diff >/dev/null 2>&1
then
    echo "You need pg-schema-diff installed"
    echo "https://github.com/stripe/pg-schema-diff/?tab=readme-ov-file#install"
    exit 1
fi


# ──────────────────────────────────────────────────────────────────────────────
# Setup the connections

# Dev
dev_password=opensesame
dev_user=postgres
dev_db=vers
dev_host=127.0.0.1
dev_dsn="postgresql://${dev_user}:${dev_password}@${dev_host}:5432/${dev_db}"


# Prod
prod_password=$PROD_POSTGRES_PASSWORD
prod_user=postgres
prod_db=vers
prod_host=vers.cwxoqiosmfyv.us-east-1.rds.amazonaws.com
prod_dsn="postgresql://${prod_user}:${prod_password}@${prod_host}:5432/${prod_db}"


# ──────────────────────────────────────────────────────────────────────────────
# Generate the diff

pg-schema-diff plan \
               --to-dsn $dev_dsn \
               --from-dsn $prod_dsn \
               
