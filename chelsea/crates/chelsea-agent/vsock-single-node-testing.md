SNE testing steps:

```
sudo -E su
./scripts/single-node.sh start

# After starting detach from tmux and exit su

./scripts/single-node/reset-ceph.sh

# Wait for it to complete
# With RUST_LOG=debug on chelsea, you can see vsocket stuff as it connects and does stuff 
./public-api.sh new 

# Copy vm id 

./public-api.sh exec --stream <vm-id> -- bash -c 'for i in {1..5}; do echo $i; sleep 1; done'
```

Outputs something like this:

```
./public-api.sh exec --stream 7c4a8519-92f6-432a-badc-9fdac695edd9 -- bash -c 'for i in {1..5}; do echo $i; sleep 1; done'
[stdout] (6b623f55-5c43-480a-923f-291831add427) 1
[stdout] (6b623f55-5c43-480a-923f-291831add427) 2
[stdout] (6b623f55-5c43-480a-923f-291831add427) 3
[stdout] (6b623f55-5c43-480a-923f-291831add427) 4
[stdout] (6b623f55-5c43-480a-923f-291831add427) 5
[exit] exec_id=6b623f55-5c43-480a-923f-291831add427 code=0
```

For stderr output:

```
./public-api.sh exec --stream 7c4a8519-92f6-432a-badc-9fdac695edd9 -- bash -c 'echo "out1"; echo "err1" >&2; sleep 1; echo "out2"; echo "err2" >&2'
[stdout] (19cfe06b-6a6a-4390-b883-dc913e973eff) out1
[stderr] (19cfe06b-6a6a-4390-b883-dc913e973eff) err1
[exit] exec_id=19cfe06b-6a6a-4390-b883-dc913e973eff code=0
```

``./commands.sh connect <vm-id>`` then look at the logs in the file ``/var/log/chelsea-agent/exec.log`` this is rotated by logD with a max size of 10mb
