sudo docker build -f crates/orchestrator/Dockerfile -t orchestrator --target checker .

CONTAINER_ID=$(sudo docker run -d -p 8090:8090 orchestrator)

sudo docker logs $CONTAINER_ID --follow ; echo "Successfully removed orchestrator docker-id: $(sudo docker rm $CONTAINER_ID -f)"
