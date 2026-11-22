-- C-068 round 2 (opsurface/round2-core.md 2, 3, 16; C-067 amended): what core last read of the backup
-- plugin's own record through the Kubernetes API (`watch::BackupEvidence`): whether the database's
-- Cluster archives its WAL through the plugin, and the plugin's recovery window for the server (the
-- ObjectStore's `status.serverRecoveryWindow`). One row, replaced at each read; a read that failed
-- keeps its error and no evidence. The watch alerts on the absence of evidence, never on a local sign.
CREATE TABLE core.backup_evidence (
    one                        boolean     PRIMARY KEY DEFAULT true CHECK (one),
    read_at                    timestamptz NOT NULL,
    error                      text,
    cluster                    text,
    archiver                   boolean,
    object_store               text,
    server                     text,
    first_recoverability_point timestamptz,
    last_successful_backup     timestamptz,
    last_failed_backup         timestamptz
);
