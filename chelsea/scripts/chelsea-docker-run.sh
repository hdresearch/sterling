CONTAINER_ID=$(sudo docker run -d --privileged --env-file crates/chelsea/.env --env RUST_LOG="chelsea=trace,orch_wg=trace,*" -p 8090:8090 chelsea)

sudo docker logs $CONTAINER_ID --follow ; echo "Successfully removed orchestrator docker-id: $(sudo docker rm $CONTAINER_ID -f)"
