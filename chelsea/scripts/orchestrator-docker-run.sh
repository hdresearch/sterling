CONTAINER_ID=$(sudo docker run -d --privileged --env-file crates/orchestrator/.env --env RUST_LOG="orchestrator=trace,orch_wg=trace,*" -p 8090:8090 orchestrator)

sudo docker logs $CONTAINER_ID --follow ; echo "Successfully removed orchestrator docker-id: $(sudo docker rm $CONTAINER_ID -f)"
