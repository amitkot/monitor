ALTER TABLE tasks ADD COLUMN hidden_at TEXT;

CREATE INDEX idx_tasks_hidden_at ON tasks(hidden_at);
