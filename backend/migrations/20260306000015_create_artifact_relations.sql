CREATE TABLE artifact_relations (
    source_id  UUID NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
    target_id  UUID NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
    relation   artifact_relation NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (source_id, target_id, relation)
);

CREATE INDEX idx_artifact_relations_target ON artifact_relations (target_id);
