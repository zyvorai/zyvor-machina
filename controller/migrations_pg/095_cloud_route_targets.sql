-- Native cloud routes: accept igw / nat / transit (parity with EC2 route targets).
ALTER TABLE cloud_routes DROP CONSTRAINT IF EXISTS cloud_routes_target_check;
ALTER TABLE cloud_routes ADD CONSTRAINT cloud_routes_target_check
    CHECK (target IN ('blackhole','local','peering','igw','nat','transit'));
