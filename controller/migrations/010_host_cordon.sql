-- Day-2: durable host cordon. `maintenance_mode` triggers the one-shot evacuate flow;
-- `schedulable = 0` (cordoned) simply stops NEW VM placement on the host while leaving
-- existing VMs running — the standard "cordon before rolling maintenance" primitive that
-- previously existed only for K8s nodes.
ALTER TABLE hosts ADD COLUMN schedulable INTEGER NOT NULL DEFAULT 1;
