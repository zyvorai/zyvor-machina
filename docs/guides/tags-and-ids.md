# Tags and EC2-style ids

## What it is
Every taggable resource (instances, volumes, snapshots, security groups, key pairs, images, network interfaces, VPCs, subnets,
instance groups, launch templates) has key/value tags and an EC2-style id such as `i-0a1b2c3d4e5f60718` or `vol-...`. The id is
derived from the resource's UUID, so it never changes, and every place that takes an instance id also accepts the EC2 id.
Tags let you group and filter resources and are what the EC2 endpoint's `tag:Key` filters read.

## Configure
Nothing to enable. Limits: 50 tags per resource, keys up to 128 characters, values up to 256. Migration `045` creates the
tables; they are applied on controller start.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X PUT $API/tags/vm/web-1 -d '{"tags":{"env":"prod","team":"web"}}'   # set tags on an instance
mc $API/tags/vm/web-1                                                       # read them
mc "$API/tags?key=env&value=prod"                                           # every resource with env=prod
mc "$API/vms?tag_key=env&tag_value=prod"                                    # filter the instance list
mc $API/ids/i-0a1b2c3d4e5f60718                                             # resolve an EC2 id
mc -X DELETE "$API/tags/vm/web-1?key=team"                                  # remove one tag
```
Volumes, security groups and key pairs return `ec2_id` in their JSON; `GET /api/v1/volumes?tag_key=&tag_value=` filters them.

## Check it works
Set a tag, then list by it and confirm the instance comes back; resolve its `ec2_id` through `/ids/` and confirm it names the
same instance. Unit tests: `cargo test -p machina-controller resource_ids api::tags` (run by the *Fleet Cloud* workflow).

## Limits
Tags are not inherited and do not drive permissions. Filtering by several tags at once is only available through the EC2
endpoint (`Filter.N`); the REST list takes one key/value pair.
