CREATE TABLE workstreams (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    workstream_id TEXT NOT NULL REFERENCES workstreams(id),
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    summary_text TEXT,
    summary_updated_at TEXT,
    summary_source TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE updates (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    source TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    message TEXT NOT NULL,
    kind TEXT,
    level TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    data TEXT
);

CREATE INDEX idx_tasks_workstream_id ON tasks(workstream_id);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_updates_task_id_seq ON updates(task_id, seq);
CREATE INDEX idx_updates_source_seq ON updates(source, seq);
CREATE INDEX idx_updates_kind_seq ON updates(kind, seq);
