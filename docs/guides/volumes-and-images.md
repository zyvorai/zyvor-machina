# Volumes and images

## What it is
Block volumes with delete-on-termination and I/O limits, volumes cloned from snapshots, and images (templates) that can be
public or private and shared with specific projects, like AMIs.

## Configure
- Volumes live in the local pool, or on Atlas when `ATLAS_ENABLED=1` (needed for snapshots and create-from-snapshot).
- Image visibility defaults to `public`. Make one private and share it with projects explicitly.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X POST $API/volumes -d '{"name":"data-1","size_gb":10,"delete_on_termination":true}'
mc -X POST $API/volumes/<id>/attach -d '{"vm":"web-1"}'                       # shows up as /dev/vdb in the guest
mc -X PUT  $API/volumes/<id>/iotune -d '{"read_iops":500,"write_iops":500}'    # 0 removes a limit; applied live
mc -X POST $API/volume-snapshots/<snap id>/create-volume -d '{"name":"data-1-copy"}'

mc -X PUT $API/templates/cirros/1/visibility -d '{"visibility":"private"}'
mc -X PUT $API/templates/cirros/1/shares/lab                                    # share with project "lab"
mc "$API/templates?project=lab"                                                 # what project lab may see
```
Launching a private image from a project that does not own it and was not given a share fails with 403 `image_not_shared`.

## Check it works
Attach a volume to a throwaway guest, read `/sys/block/vdb` there, set an I/O limit and confirm the host reports it
(`virsh blkdeviotune <domain> vdb`). Delete the instance and confirm a flagged volume is gone. Unit tests:
`cargo test -p machina-controller iotune_tests image_access_tests visibility_tests` and
`cargo test -p machina-core iotune_tests`.

## Limits
I/O limits are not re-applied on a later attach. Snapshots exist for Atlas-backed volumes only. If a backend delete fails on
termination the volume is kept (detached) and the instance delete still succeeds. No disk encryption.
