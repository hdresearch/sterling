# How to run orchestrator and chelsea on the same node
All instructions in this document are current as of the commits on branch `js/single-node`.

## Env setup
The first step is to create a .env for chelsea. `cp .env.example .env` and fill in the mising AWS values.  
The env variables for orchestrator are embedded in `single-node.sh`, so we will not be using a .env file for those.  

## Postgres setup
Next, set up a postgres database and seed it. From `pg/scripts`:
1) If one already exists, `./reset-dev-db.sh`
2) `./setup-dev-db.sh` (Ignore NULL column violations from `_seed.sql` for now; `vms` and `domains` need not be seeded, and in fact feel free to remove these. We don't really want fake data atp.)
3) `./insert-node.sh <instance-id>`, where `<instance-id>` is the AWS EC2 instance ID of the machine in question

## Actually starting orch+chelsea
Next, run `./scripts/pre-single-node.sh`. This should install all dependencies, grab the kernel, grab Ceph credentials, etc. This only needs to be run once.  

To start orch and chelsea, `./scripts/single-node.sh`. You will use this each time you wish to start orch+chelsea. At this point, stdout should stabilize. You will see one warning about unknown node status for the instance you inserted in step 3 above:
```
2025-10-21T00:00:40.998955Z  WARN action execution{id="healthcheck" exec_id=730a1091-548d-48b5-a772-a122138c5882}: orchestrator::action::health_check: unknown node status, fetching info from cloud provider node_id=78e53778-fcb1-431a-a247-38c8249f7002
```
But after that, the output should be stable. At this point, chelsea and orch are both theoretically running.

## Testing chelsea
You can fetch chelsea's IP address by inspecting the address of its Wireguard interface. This is a constant, but this is a good way to confirm the interface was created.
```bash
ip addr show wgchelsea
143: wgchelsea: <POINTOPOINT,NOARP,UP,LOWER_UP> mtu 1420 qdisc noqueue state UNKNOWN group default qlen 1000
    link/none
    inet6 fd00:fe11:deed::1/128 scope global
       valid_lft forever preferred_lft forever
```
You can now curl chelsea using the IPv6 address and its port (using default env, this should be 8111)
```bash
curl [fd00:fe11:deed::1]:8111/api/system/telemetry | jq .
  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
                                 Dload  Upload   Total   Spent    Left  Speed
100   350  100   350    0     0   200k      0 --:--:-- --:--:-- --:--:--  341k
{
  "ram": {
    ...
  },
  "cpu": {
    ...
  },
  ...
}
```
`./api.sh` is partially broken atm, since all VM creation requests `new_root`, `branch`, and `run-commit` require wireguard configs in the payload. Generating random configs is totally valid; see `./scripts/test-wg-endpoints.tcl`. As a matter of fact, this script should still work as expected. Just be sure to update the hostname:
```tcl
set hostname {[fd00:fe11:deed::1]:8111}
```
(Reminder that if the `new_root` request hangs for > 10 seconds, it's likely because of a security group error. `ceph --user chelsea status` to confirm; this should return info in no more than a few seconds.)  
The expected types for these routes can be derived from `crates/chelsea_server2/src/routes/vm.rs`. 

## Testing orchestrator
Where I'm leaving off on October 20 is that I currently do not know what values for the `Authorization` header are valid. But until then, you can check the orch IP similarly to chelsea:
```bash
ip addr show wgorchestrator
149: wgorchestrator: <POINTOPOINT,NOARP,UP,LOWER_UP> mtu 1420 qdisc noqueue state UNKNOWN group default qlen 1000
    link/none
    inet6 fd00:fe11:deed::ffff/128 scope global
       valid_lft forever preferred_lft forever
```
And you are now able to curl it. For an example, let's try the `GET /api/v1/node/{id}/vms` route:
```bash
psql postgresql://postgres:opensesame@localhost:5432/vers -c "SELECT node_id FROM nodes;"
               node_id
--------------------------------------
 78e53778-fcb1-431a-a247-38c8249f7002
(1 row)

curl [fd00:fe11:deed::ffff]:3000/api/v1/node/78e53778-fcb1-431a-a247-38c8249f7002/vms -v
...
{"error":"Missing or invalid Authorization header","success":false}
```
And this is as far as I've gotten. Feel free to update this doc as you make progress!
