-- When the deploy ramdisk finished writing the image. A machine whose image is written
-- must stop being offered the ramdisk when it PXE-boots again before it has joined.
ALTER TABLE baremetal_deployments ADD COLUMN written_at TEXT NOT NULL DEFAULT '';
