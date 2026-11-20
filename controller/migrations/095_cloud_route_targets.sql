-- Native cloud routes: accept igw / nat / transit (parity with EC2 route targets).
CREATE TABLE cloud_routes__new (
    id TEXT PRIMARY KEY NOT NULL,
    vpc_id TEXT NOT NULL REFERENCES cloud_vpcs(id) ON DELETE CASCADE,
    destination TEXT NOT NULL,
    target TEXT NOT NULL CHECK(target IN ('blackhole','local','peering','igw','nat','transit')),
    target_id TEXT,
    UNIQUE(vpc_id, destination)
);
INSERT INTO cloud_routes__new (id, vpc_id, destination, target, target_id)
SELECT id, vpc_id, destination, target, target_id FROM cloud_routes;
DROP TABLE cloud_routes;
ALTER TABLE cloud_routes__new RENAME TO cloud_routes;
