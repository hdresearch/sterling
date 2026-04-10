sudo docker build -f crates/chelsea/Dockerfile -t chelsea --target checker .

CONTAINER_ID=$(sudo docker run -d -p 8090:8090 chelsea)

sudo docker logs $CONTAINER_ID --follow ; echo "Successfully removed orchestrator docker-id: $(sudo docker rm $CONTAINER_ID -f)"
