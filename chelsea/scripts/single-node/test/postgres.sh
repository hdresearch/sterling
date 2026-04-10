#!/bin/bash

echo 'select api_key_id, label, is_active from api_keys' | psql $DATABASE_URL --expanded >> /dev/null

if [ $? -eq 0 ]; then
    echo "✓ Postgres is up"
else
    echo "⚠ Postgres failed!"
fi

