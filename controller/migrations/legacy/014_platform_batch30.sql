-- Batch 30: template marketplace + ISO approval workflow

ALTER TABLE content_images ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT 'Custom Appliances';
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS submitted_by TEXT;
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS approved_by TEXT;
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS approved_at TIMESTAMPTZ;
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS rejected_reason TEXT;

ALTER TABLE templates ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT 'Linux';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS featured BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE templates ADD COLUMN IF NOT EXISTS marketplace BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE templates ADD COLUMN IF NOT EXISTS icon TEXT;

CREATE INDEX IF NOT EXISTS idx_content_images_status ON content_images(status);
CREATE INDEX IF NOT EXISTS idx_templates_marketplace ON templates(marketplace, featured);
