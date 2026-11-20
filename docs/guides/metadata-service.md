# Instance metadata service

## What it is
The 169.254.169.254 endpoint guests use to learn about themselves, as on EC2: `instance-id`, `hostname`, `local-ipv4`,
`instance-type`, `ami-id`, `placement/availability-zone`, `public-keys/0/openssh-key`, `/latest/user-data` and
`/latest/dynamic/instance-identity/document`, also under dated versions such as `/2009-04-04/` for cirros and other datasources.

## Configure
On by default. The controller pushes one entry per instance to its host's agent every 30 s; the agent serves them on port 8169
and an nft rule (`table ip machina_imds`) redirects `169.254.169.254:80` to it. `MACHINA_IMDS=0` turns it off (agent and
controller), `MACHINA_IMDS_PORT` changes the port. Guests need a route to the link-local address: the default NAT network
provides one; isolated cloud subnets need a route to it via the subnet's `.1`.

## Use
Inside a guest:
```bash
curl http://169.254.169.254/latest/meta-data/instance-id
curl http://169.254.169.254/latest/meta-data/public-keys/0/openssh-key
curl http://169.254.169.254/latest/user-data
```
The answer is chosen by the request's source address, so a guest sees only its own entry; an unknown address gets 404.

## Check it works
In a guest the instance-id must match the id the API returns. On the host, `sudo nft list table ip machina_imds` shows the
redirect. Unit tests: `cargo test -p machina-agent imds::tests` and `cargo test -p machina-controller engine::imds`.

## Limits
Needs an address the controller knows for the guest (DHCP lease or guest agent). No IMDSv2 token and no hop limit: anything
running in the guest, including a vulnerable web application, can read user data, so keep long-lived secrets out of it. Tags
are not exposed.
