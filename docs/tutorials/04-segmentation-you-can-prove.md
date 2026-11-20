# Tutorial: segmentation you can prove

Allow only web VMs to reach the database on 5432, test the rule before it bites, replay it against real traffic history, and
export signed evidence an auditor can verify. About 20 minutes with two throwaway VMs. Uses the VM network policy engine
(Cilium policy schema, enforced by Machina's own eBPF); the full reference is [vm-network-policy](../ebpf/vm-network-policy.md).

You need two running VMs (`tut-web`, `tut-db`) and `machinactl` (add `--fleet` to go through the controller).

## 1. Label them
```bash
machinactl vm label tut-web app=web
machinactl vm label tut-db  app=db
machinactl netpol endpoints            # both appear with identity and labels
```

## 2. Write the policy
```yaml
# db-from-web.yaml
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata: { name: tut-db-from-web }
spec:
  description: Only web VMs reach the database, on 5432/TCP
  endpointSelector: { matchLabels: { app: db } }
  ingress:
    - fromEndpoints: [ { matchLabels: { app: web } } ]
      toPorts: [ { ports: [ { port: "5432", protocol: TCP } ] } ]
```

## 3. Check before you apply
```bash
machinactl netpol validate -f db-from-web.yaml      # schema check + which VMs and rules it selects
machinactl netpol replay   -f db-from-web.yaml      # what it would have done to the last 7 days of connections
machinactl netpol test --from tut-web --to tut-db --port 5432   # allowed
machinactl netpol test --from tut-web --to tut-db --port 22     # denied
```
`replay` lists connections that would break and ones that would newly be allowed, so you see the blast radius first.

## 4. Apply and look at the traffic
```bash
machinactl netpol apply -f db-from-web.yaml
```
Open Platform > Security > VM Network Policies > **Flows** to watch the connections arrive, allowed and dropped. Enforcement
is lease-gated and fails open by design ([security notes](../buyers/security-and-compliance.md)), so check
`machinactl netpol status` reports enforcement as active.

## 5. Evidence for the audit
```bash
machinactl netpol evidence -o md   --out evidence.md      # readable
machinactl netpol evidence -o json --out evidence.json    # signed, verifiable
machinactl netpol evidence verify evidence.json     # checks the signature; pass the fleet CA to pin it
```

## You should see
`test` allowing 5432 and denying 22, the Flows view showing drops for anything else, and evidence that verifies.

## Clean up
`machinactl netpol delete tut-db-from-web`, then delete the VMs.

## Know the edges
Policies are per VM and need the VM edge on its host; cross-host policy depends on the overlay. Wording like "least privilege"
is earned by `netpol learn`, which proposes policy from observed flows: review it, never apply it blind.
