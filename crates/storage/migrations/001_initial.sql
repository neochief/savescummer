CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE games (id TEXT PRIMARY KEY, body TEXT NOT NULL);
CREATE TABLE snapshots (id TEXT PRIMARY KEY, game_id TEXT NOT NULL, body TEXT NOT NULL);
CREATE INDEX snapshots_game ON snapshots(game_id);
CREATE TABLE history (id TEXT PRIMARY KEY, sequence INTEGER NOT NULL UNIQUE, game_id TEXT NOT NULL, body TEXT NOT NULL);
CREATE INDEX history_game ON history(game_id, sequence);
CREATE TABLE operations (id TEXT PRIMARY KEY, request_id TEXT NOT NULL UNIQUE, game_id TEXT NOT NULL, body TEXT NOT NULL);
CREATE INDEX operations_game ON operations(game_id);
PRAGMA user_version = 1;
