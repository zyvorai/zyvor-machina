-- Retire superseded marketplace OS templates (replaced in template_catalog CATALOG).
DELETE FROM templates
WHERE marketplace = TRUE
  AND name IN (
    'ubuntu-22.04',
    'debian-12',
    'centos-stream-9',
    'rocky-9',
    'alma-9',
    'fedora-40',
    'rhel-9'
  );
