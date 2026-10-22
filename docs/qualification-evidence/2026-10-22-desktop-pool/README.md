# Desktop pools (claim C63): live run, 2026-10-22

One lab host (Ubuntu 26.04.1, libvirt 12.0.0), controller + agent + daemon built as `0.3.1-lab2`, then `0.3.1-lab3`. The template was a stopped
256 MB VM with a blank 1 GB disk (no guest OS), pool `qual-dp` of 2 linked clones.

- `lab2`: create (2 clones), checkout (powers the desktop on; a second checkout by the same user returns the same desktop),
  scale up to 3 and down to 1 all worked. Release deleted the clone's VM but left its qcow2, so the re-clone failed with "Refusing to
  overwrite existing disk" and the desktop stayed in `cloning`; scale-down left orphan disks too.
- `lab3` (includes the fix, #203): create, checkout, release (the desktop went `needs_clone` then `ready` within a minute),
  scale down and `DELETE ...?delete_vms=true` all worked, and no `qual-dp-*` disk remained afterwards.

Not shown: a guest that boots, Windows or multi-session, more than one host, or any connection broker (there is none).
