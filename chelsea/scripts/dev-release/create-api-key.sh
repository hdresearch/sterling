# From postgres seed migration
USER_ID=9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9
ORG_ID=2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d

# From dev config ini file
ORCH_ADMIN_API_KEY=3114e635-285c-4c83-be5c-9a68542f6d25

curl -sS -H "Host: api.vers.sh" \
    -H "Authorization: Bearer $ORCH_ADMIN_API_KEY" \
    -H "Content-Type: application/json" \
    -d "{\"user_id\": \"${USER_ID}\", \"org_id\": \"${ORG_ID}\", \"label\": \"generated\"}" \
    "http://[fd00:fe11:deed::ffff]:8090/api/v1/admin/api_key" \
    | jq -r .api_key