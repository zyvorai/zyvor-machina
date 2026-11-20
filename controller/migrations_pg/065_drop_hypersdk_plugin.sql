-- The HyperSDK (hypervisord) bulk-migration integration was removed.
DELETE FROM platform_plugins WHERE slug = 'hypersdk';
