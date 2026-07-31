//! Capabilities persistence — SQLite adapters for skill config, settings, the
//! autonomy envelope, and the deep-model configuration, over the shared handle.

use endora_kernel::RepositoryError;
use endora_persistence::{Db, backend, corrupt};
use rusqlite::{OptionalExtension, params};

use crate::application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilitySettingsRepository, ConfigWrite, DeepModel,
    DeepModelRepository, McpServer, McpServerRegistry, McpTransport, ModelSlot, ModelTuneSchedule,
    ModelTuneScheduleRepository, Sampling, StandingTrouble, StandingTroubleRepository, TargetAlias,
    TargetAliasRepository,
};

/// Creates the capabilities config tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
/// Applies this context's schema.
///
/// **Production does not call this.** The composition root shares one database whose
/// schema comes from `endora-infrastructure`'s `SCHEMA`, and this exists so the context's
/// own tests can stand a store up alone. A table added here and not there is created in
/// every test and in no real database — which is exactly what happened to `config_writes`,
/// and why the tables below must be kept in step with that `SCHEMA`.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_config (
                id                TEXT PRIMARY KEY,
                enabled           INTEGER NOT NULL,
                open_irreversible INTEGER NOT NULL DEFAULT 0,
                confirm           INTEGER NOT NULL DEFAULT 0
            ) STRICT;
            CREATE TABLE IF NOT EXISTS capability_settings (
                capability_id TEXT NOT NULL,
                key           TEXT NOT NULL,
                value         TEXT NOT NULL,
                PRIMARY KEY (capability_id, key)
            ) STRICT;
            CREATE TABLE IF NOT EXISTS autonomy_envelope (
                id                 INTEGER PRIMARY KEY CHECK (id = 0),
                auto_external      INTEGER NOT NULL,
                auto_consequential INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS deep_model (
                id       INTEGER PRIMARY KEY CHECK (id = 0),
                url      TEXT NOT NULL,
                model    TEXT NOT NULL,
                api_key  TEXT NOT NULL,
                escalate INTEGER NOT NULL DEFAULT 0
            ) STRICT;
            CREATE TABLE IF NOT EXISTS butler_model_config (
                id            INTEGER PRIMARY KEY CHECK (id = 0),
                base_url      TEXT NOT NULL,
                api_key       TEXT NOT NULL,
                mixture       INTEGER NOT NULL,
                single_model  TEXT NOT NULL,
                single_temp   REAL,
                single_top_p  REAL,
                single_top_k  INTEGER,
                single_repeat REAL,
                router_model  TEXT NOT NULL,
                router_temp   REAL,
                router_top_p  REAL,
                router_top_k  INTEGER,
                router_repeat REAL,
                synth_model   TEXT NOT NULL,
                synth_temp    REAL,
                synth_top_p   REAL,
                synth_top_k   INTEGER,
                synth_repeat  REAL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS model_tune_schedule (
                id       INTEGER PRIMARY KEY CHECK (id = 0),
                enabled  INTEGER NOT NULL,
                hour_utc INTEGER NOT NULL,
                last_ms  INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS standing_trouble (
                server   TEXT NOT NULL,
                thing    TEXT NOT NULL,
                trouble  TEXT NOT NULL,
                since_ms INTEGER NOT NULL,
                accepted INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (server, thing, trouble)
            ) STRICT;
            CREATE TABLE IF NOT EXISTS target_aliases (
                server TEXT NOT NULL,
                said   TEXT NOT NULL,
                means  TEXT NOT NULL,
                PRIMARY KEY (server, said)
            ) STRICT;
            CREATE TABLE IF NOT EXISTS config_writes (
                id      TEXT PRIMARY KEY,
                at_ms   INTEGER NOT NULL,
                server  TEXT NOT NULL,
                target  TEXT NOT NULL,
                added   TEXT NOT NULL,
                was     TEXT NOT NULL,
                undone  INTEGER NOT NULL DEFAULT 0,
                kind    TEXT NOT NULL DEFAULT 'name'
            ) STRICT;
            CREATE TABLE IF NOT EXISTS mcp_servers (
                name    TEXT PRIMARY KEY,
                kind    TEXT NOT NULL,
                command TEXT NOT NULL DEFAULT '',
                args    TEXT NOT NULL DEFAULT '',
                url     TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                env     TEXT NOT NULL DEFAULT '',
                auth    TEXT NOT NULL DEFAULT '',
                trust_all INTEGER NOT NULL DEFAULT 0,
                reader_tool TEXT NOT NULL DEFAULT ''
            ) STRICT;",
        )
        .map_err(backend)?;
    // Additive migrations for databases created before these columns existed —
    // the irreversible-opener (ADR 0051) and the ask-first override. Ignore the
    // error when a column is already present (a fresh DB gets it from the CREATE).
    let _ = db.lock()?.execute(
        "ALTER TABLE capability_config ADD COLUMN open_irreversible INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = db.lock()?.execute(
        "ALTER TABLE capability_config ADD COLUMN confirm INTEGER NOT NULL DEFAULT 0",
        [],
    );
    // Per-server credentials: a child-process environment (stdio) and a bearer token
    // (http). Secrets — stored here, never returned by the API.
    let _ = db.lock()?.execute(
        "ALTER TABLE mcp_servers ADD COLUMN env TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = db.lock()?.execute(
        "ALTER TABLE mcp_servers ADD COLUMN auth TEXT NOT NULL DEFAULT ''",
        [],
    );
    // Auto-allow a server's tools on connect (default on). Opened tools are still
    // Block→Confirm, so this never removes the ask-before-each-use safety net.
    let _ = db.lock()?.execute(
        "ALTER TABLE mcp_servers ADD COLUMN trust_all INTEGER NOT NULL DEFAULT 0",
        [],
    );
    // Which tool reads this server's state (ADR 0054). Existing rows read back blank —
    // nobody has nominated one — and blank means "no read-back for this server", which
    // is the same honest default a server Endora knows nothing about already gets.
    let _ = db.lock()?.execute(
        "ALTER TABLE mcp_servers ADD COLUMN reader_tool TEXT NOT NULL DEFAULT ''",
        [],
    );
    Ok(())
}

/// SQLite-backed store for the four capabilities config repositories over the
/// shared connection handle.
pub struct ConfigStore {
    db: Db,
}

impl ConfigStore {
    /// Builds the store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl DeepModelRepository for ConfigStore {
    fn get(&self) -> Result<Option<DeepModel>, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT url, model, api_key, escalate FROM deep_model WHERE id = 0",
                [],
                |r| {
                    Ok(DeepModel {
                        url: r.get(0)?,
                        model: r.get(1)?,
                        api_key: r.get(2)?,
                        escalate: r.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn set(&self, model: &DeepModel) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO deep_model (id, url, model, api_key, escalate) \
                 VALUES (0, ?1, ?2, ?3, ?4)",
                params![model.url, model.model, model.api_key, model.escalate],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl ButlerModelConfigRepository for ConfigStore {
    fn get(&self) -> Result<Option<ButlerModelConfig>, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT base_url, api_key, mixture, \
                 single_model, single_temp, single_top_p, single_top_k, single_repeat, \
                 router_model, router_temp, router_top_p, router_top_k, router_repeat, \
                 synth_model, synth_temp, synth_top_p, synth_top_k, synth_repeat \
                 FROM butler_model_config WHERE id = 0",
                [],
                |r| {
                    Ok(ButlerModelConfig {
                        base_url: r.get(0)?,
                        api_key: r.get(1)?,
                        mixture: r.get::<_, i64>(2)? != 0,
                        single: ModelSlot {
                            model: r.get(3)?,
                            sampling: Sampling {
                                temperature: r.get(4)?,
                                top_p: r.get(5)?,
                                top_k: r.get(6)?,
                                repeat_penalty: r.get(7)?,
                            },
                        },
                        router: ModelSlot {
                            model: r.get(8)?,
                            sampling: Sampling {
                                temperature: r.get(9)?,
                                top_p: r.get(10)?,
                                top_k: r.get(11)?,
                                repeat_penalty: r.get(12)?,
                            },
                        },
                        synth: ModelSlot {
                            model: r.get(13)?,
                            sampling: Sampling {
                                temperature: r.get(14)?,
                                top_p: r.get(15)?,
                                top_k: r.get(16)?,
                                repeat_penalty: r.get(17)?,
                            },
                        },
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn set(&self, c: &ButlerModelConfig) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO butler_model_config (\
                 id, base_url, api_key, mixture, \
                 single_model, single_temp, single_top_p, single_top_k, single_repeat, \
                 router_model, router_temp, router_top_p, router_top_k, router_repeat, \
                 synth_model, synth_temp, synth_top_p, synth_top_k, synth_repeat) \
                 VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                 ?16, ?17, ?18)",
                params![
                    c.base_url,
                    c.api_key,
                    i64::from(c.mixture),
                    c.single.model,
                    c.single.sampling.temperature,
                    c.single.sampling.top_p,
                    c.single.sampling.top_k,
                    c.single.sampling.repeat_penalty,
                    c.router.model,
                    c.router.sampling.temperature,
                    c.router.sampling.top_p,
                    c.router.sampling.top_k,
                    c.router.sampling.repeat_penalty,
                    c.synth.model,
                    c.synth.sampling.temperature,
                    c.synth.sampling.top_p,
                    c.synth.sampling.top_k,
                    c.synth.sampling.repeat_penalty,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl ModelTuneScheduleRepository for ConfigStore {
    fn get(&self) -> Result<ModelTuneSchedule, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT enabled, hour_utc, last_ms FROM model_tune_schedule WHERE id = 0",
                [],
                |r| {
                    Ok(ModelTuneSchedule {
                        enabled: r.get::<_, i64>(0)? != 0,
                        hour_utc: r.get::<_, i64>(1)? as u8,
                        last_ms: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(backend)
            .map(|opt| opt.unwrap_or_else(ModelTuneSchedule::disabled_default))
    }

    fn set(&self, s: &ModelTuneSchedule) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO model_tune_schedule (id, enabled, hour_utc, last_ms) \
                 VALUES (0, ?1, ?2, ?3)",
                params![i64::from(s.enabled), i64::from(s.hour_utc), s.last_ms],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl AutonomyEnvelopeRepository for ConfigStore {
    fn get(&self) -> Result<AutonomyEnvelope, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT auto_external, auto_consequential FROM autonomy_envelope WHERE id = 0",
                [],
                |r| {
                    Ok(AutonomyEnvelope {
                        auto_external: r.get::<_, i64>(0)? != 0,
                        auto_consequential: r.get::<_, i64>(1)? != 0,
                    })
                },
            )
            .optional()
            .map_err(backend)
            .map(Option::unwrap_or_default)
    }

    fn set(&self, envelope: &AutonomyEnvelope) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO autonomy_envelope (id, auto_external, auto_consequential) \
                 VALUES (0, ?1, ?2)",
                params![
                    i64::from(envelope.auto_external),
                    i64::from(envelope.auto_consequential)
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl CapabilitySettingsRepository for ConfigStore {
    fn all_settings(&self) -> Result<Vec<(String, String, String)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT capability_id, key, value FROM capability_settings")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn set_setting(
        &self,
        capability_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO capability_settings (capability_id, key, value) \
                 VALUES (?1, ?2, ?3)",
                params![capability_id, key, value],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl CapabilityConfigRepository for ConfigStore {
    fn enabled_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, enabled FROM capability_config")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RepositoryError> {
        // Upsert only the enabled column; a new row defaults the opener to closed,
        // and an existing row keeps its opener flag untouched.
        self.db
            .lock()?
            .execute(
                "INSERT INTO capability_config (id, enabled, open_irreversible) VALUES (?1, ?2, 0) \
                 ON CONFLICT(id) DO UPDATE SET enabled = excluded.enabled",
                params![id, i64::from(enabled)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn opened_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, open_irreversible FROM capability_config")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn set_open_irreversible(&self, id: &str, opened: bool) -> Result<(), RepositoryError> {
        // Upsert only the opener column; a new row defaults enabled to on (the
        // built-in default), and an existing row keeps its enabled flag untouched.
        self.db
            .lock()?
            .execute(
                "INSERT INTO capability_config (id, enabled, open_irreversible) VALUES (?1, 1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET open_irreversible = excluded.open_irreversible",
                params![id, i64::from(opened)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn confirm_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, confirm FROM capability_config")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn set_confirm(&self, id: &str, confirm: bool) -> Result<(), RepositoryError> {
        // Upsert only the confirm column; a new row defaults enabled on and the
        // opener closed, and an existing row keeps its other flags untouched.
        self.db
            .lock()?
            .execute(
                "INSERT INTO capability_config (id, enabled, confirm) VALUES (?1, 1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET confirm = excluded.confirm",
                params![id, i64::from(confirm)],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl crate::application::ConfigWriteLog for ConfigStore {
    fn record(&self, write: &ConfigWrite) -> Result<(), RepositoryError> {
        // `was` is stored as JSON: it is a list, and flattening it to a delimited string
        // would corrupt any name containing the delimiter — which is exactly the sort of
        // detail an undo cannot afford to get wrong.
        let was = serde_json::to_string(&write.was).map_err(|e| corrupt(e.to_string()))?;
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO config_writes \
                 (id, at_ms, server, target, added, was, undone, kind) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    write.id.to_string(),
                    write.at_ms,
                    write.server,
                    write.target,
                    write.added,
                    was,
                    i64::from(write.undone),
                    write.kind.as_str(),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn writes(&self, limit: usize) -> Result<Vec<ConfigWrite>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, at_ms, server, target, added, was, undone, kind \
                 FROM config_writes ORDER BY at_ms DESC LIMIT ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at_ms, server, target, added, was, undone, kind) = row.map_err(backend)?;
            out.push(ConfigWrite {
                id: id.parse().map_err(|_| corrupt("unreadable id"))?,
                at_ms,
                server,
                target,
                added,
                was: serde_json::from_str(&was).unwrap_or_default(),
                undone: undone != 0,
                kind: crate::domain::WriteKind::read(&kind),
            });
        }
        Ok(out)
    }

    fn write(&self, id: u128) -> Result<Option<ConfigWrite>, RepositoryError> {
        Ok(self.writes(usize::MAX)?.into_iter().find(|w| w.id == id))
    }

    fn mark_undone(&self, id: u128) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE config_writes SET undone = 1 WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl TargetAliasRepository for ConfigStore {
    fn aliases(&self) -> Result<Vec<TargetAlias>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT server, said, means FROM target_aliases ORDER BY server, said")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (server, said, means) = row.map_err(backend)?;
            out.push(TargetAlias::new(&server, &said, &means).map_err(corrupt)?);
        }
        Ok(out)
    }

    fn set_alias(&self, alias: &TargetAlias) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO target_aliases (server, said, means) VALUES (?1, ?2, ?3)",
                params![alias.server, alias.said, alias.means],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn forget_alias(&self, server: &str, said: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "DELETE FROM target_aliases WHERE server = ?1 AND said = ?2",
                params![server, said],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl StandingTroubleRepository for ConfigStore {
    fn note_trouble(&self, trouble: &StandingTrouble) -> Result<(), RepositoryError> {
        // `DO NOTHING` is the whole design: this runs every time the thing is seen in
        // trouble, and only the first one sets the clock. Overwriting `since_ms` would
        // reset the duration on every heartbeat and no problem would ever get old enough
        // to be worth saying.
        self.db
            .lock()?
            .execute(
                "INSERT INTO standing_trouble (server, thing, trouble, since_ms, accepted) \
                 VALUES (?1, ?2, ?3, ?4, 0) ON CONFLICT DO NOTHING",
                params![
                    trouble.server,
                    trouble.thing,
                    trouble.trouble,
                    trouble.since_ms
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn clear_trouble(&self, server: &str, thing: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "DELETE FROM standing_trouble WHERE server = ?1 AND thing = ?2",
                params![server, thing],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn troubles(&self) -> Result<Vec<StandingTrouble>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT server, thing, trouble, since_ms, accepted FROM standing_trouble \
                 ORDER BY since_ms ASC, thing ASC",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(StandingTrouble {
                    server: r.get(0)?,
                    thing: r.get(1)?,
                    trouble: r.get(2)?,
                    since_ms: r.get(3)?,
                    accepted: r.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn accept_trouble(&self, server: &str, thing: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE standing_trouble SET accepted = 1 WHERE server = ?1 AND thing = ?2",
                params![server, thing],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl McpServerRegistry for ConfigStore {
    fn list(&self) -> Result<Vec<McpServer>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT name, kind, command, args, url, enabled, env, auth, trust_all, reader_tool \
                 FROM mcp_servers ORDER BY name",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?, // name
                    r.get::<_, String>(1)?, // kind
                    r.get::<_, String>(2)?, // command
                    r.get::<_, String>(3)?, // args (JSON array)
                    r.get::<_, String>(4)?, // url
                    r.get::<_, i64>(5)? != 0,
                    r.get::<_, String>(6)?,   // env (JSON object) — secret
                    r.get::<_, String>(7)?,   // auth (bearer token) — secret
                    r.get::<_, i64>(8)? != 0, // trust_all
                    r.get::<_, String>(9)?,   // reader_tool (ADR 0054)
                ))
            })
            .map_err(backend)?;
        let mut servers = Vec::new();
        for row in rows {
            let (
                name,
                kind,
                command,
                args_json,
                url,
                enabled,
                env_json,
                auth,
                trust_all,
                reader_tool,
            ) = row.map_err(backend)?;
            let transport = match kind.as_str() {
                "http" => McpTransport::Http { url, auth },
                // Default anything else to stdio — the only other variant we write.
                _ => {
                    let args: Vec<String> = if args_json.is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str(&args_json).map_err(corrupt)?
                    };
                    let env = if env_json.is_empty() {
                        std::collections::BTreeMap::new()
                    } else {
                        serde_json::from_str(&env_json).map_err(corrupt)?
                    };
                    McpTransport::Stdio { command, args, env }
                }
            };
            servers.push(McpServer {
                name,
                transport,
                enabled,
                trust_all,
                reader_tool,
            });
        }
        Ok(servers)
    }

    fn register(&self, server: &McpServer) -> Result<(), RepositoryError> {
        let (kind, command, args_json, url, env_json, auth) = match &server.transport {
            McpTransport::Stdio { command, args, env } => (
                "stdio",
                command.as_str(),
                serde_json::to_string(args).map_err(backend)?,
                String::new(),
                serde_json::to_string(env).map_err(backend)?,
                String::new(),
            ),
            McpTransport::Http { url, auth } => (
                "http",
                "",
                String::new(),
                url.clone(),
                String::new(),
                auth.clone(),
            ),
        };
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO mcp_servers \
                 (name, kind, command, args, url, enabled, env, auth, trust_all, reader_tool) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    server.name,
                    kind,
                    command,
                    args_json,
                    url,
                    i64::from(server.enabled),
                    env_json,
                    auth,
                    i64::from(server.trust_all),
                    server.reader_tool,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE mcp_servers SET enabled = ?2 WHERE name = ?1",
                params![name, i64::from(enabled)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_trust_all(&self, name: &str, trust_all: bool) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE mcp_servers SET trust_all = ?2 WHERE name = ?1",
                params![name, i64::from(trust_all)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute("DELETE FROM mcp_servers WHERE name = ?1", params![name])
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigStore, migrate};
    use crate::application::{
        AutonomyEnvelope, AutonomyEnvelopeRepository, CapabilityConfigRepository,
        CapabilitySettingsRepository,
    };
    use endora_persistence::Db;

    #[test]
    fn envelope_defaults_then_round_trips() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);
        assert_eq!(
            AutonomyEnvelopeRepository::get(&store).unwrap(),
            AutonomyEnvelope::default()
        );
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };
        AutonomyEnvelopeRepository::set(&store, &widened).unwrap();
        assert_eq!(AutonomyEnvelopeRepository::get(&store).unwrap(), widened);
    }

    #[test]
    fn settings_and_config_round_trip() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);
        store.set_setting("weather", "units", "metric").unwrap();
        assert_eq!(
            store.all_settings().unwrap(),
            vec![(
                "weather".to_owned(),
                "units".to_owned(),
                "metric".to_owned()
            )]
        );
        store.set_enabled("news", false).unwrap();
        assert_eq!(
            store.enabled_overrides().unwrap(),
            vec![("news".to_owned(), false)]
        );
    }

    #[test]
    fn mcp_servers_round_trip_upsert_toggle_and_remove() {
        use crate::application::{McpServer, McpServerRegistry, McpTransport};

        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);

        // Empty to start.
        assert!(store.list().unwrap().is_empty());

        // Register a stdio server (with args) and an http one.
        let fs = McpServer::stdio(
            "filesystem",
            "npx",
            ["-y".to_owned(), "server-fs".to_owned()],
        )
        .unwrap();
        let cal = McpServer::http("calendar", "https://cal.example").unwrap();
        store.register(&fs).unwrap();
        store.register(&cal).unwrap();

        // Both come back, ordered by name, with transports intact.
        let listed = store.list().unwrap();
        assert_eq!(listed, vec![cal.clone(), fs.clone()]);
        assert!(matches!(
            &listed[1].transport,
            McpTransport::Stdio { command, args, .. }
                if command == "npx" && args == &["-y".to_owned(), "server-fs".to_owned()]
        ));

        // Upsert by name replaces the transport in place (no duplicate row).
        let fs2 = McpServer::http("filesystem", "https://fs.example").unwrap();
        store.register(&fs2).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[1].transport,
            McpTransport::Http {
                url: "https://fs.example".to_owned(),
                auth: String::new()
            }
        );

        // Toggle enabled without touching the transport. (Disambiguated: both this
        // registry and CapabilityConfigRepository expose `set_enabled`.)
        McpServerRegistry::set_enabled(&store, "calendar", false).unwrap();
        let cal_row = store
            .list()
            .unwrap()
            .into_iter()
            .find(|s| s.name == "calendar")
            .unwrap();
        assert!(!cal_row.enabled);
        assert_eq!(
            cal_row.transport,
            McpTransport::Http {
                url: "https://cal.example".to_owned(),
                auth: String::new()
            }
        );

        // Remove is idempotent.
        store.remove("calendar").unwrap();
        store.remove("calendar").unwrap();
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
            vec!["filesystem".to_owned()]
        );
    }

    #[test]
    fn mcp_trust_all_defaults_off_round_trips_and_toggles() {
        use crate::application::{McpServer, McpServerRegistry, McpTransport};

        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);

        // A freshly registered server opens NOTHING. It used to open everything, and the
        // doc claimed opened tools still confirm each use — which stopped being true the
        // moment the person widened the envelope. Live, to "Good morning": `HassBroadcast`
        // played audio through the house. Nobody had chosen to open it (ADR 0054).
        let ha = McpServer::http("home-assistant", "https://ha.local/mcp_server/sse").unwrap();
        assert!(
            !ha.trust_all,
            "adding a server must not open every tool it happens to expose"
        );
        store.register(&ha).unwrap();
        assert!(
            !store.list().unwrap()[0].trust_all,
            "and it stays closed once stored"
        );

        // Turning it ON is a decision, and it persists — leaving the transport untouched.
        McpServerRegistry::set_trust_all(&store, "home-assistant", true).unwrap();
        let row = store.list().unwrap().into_iter().next().unwrap();
        assert!(row.trust_all);
        assert_eq!(
            row.transport,
            McpTransport::Http {
                url: "https://ha.local/mcp_server/sse".to_owned(),
                auth: String::new(),
            }
        );

        // And back off.
        McpServerRegistry::set_trust_all(&store, "home-assistant", false).unwrap();
        assert!(!store.list().unwrap()[0].trust_all);
    }

    #[test]
    fn opener_round_trips_and_does_not_clobber_enabled() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);

        // Closed by default: no rows.
        assert!(store.opened_overrides().unwrap().is_empty());

        // Open one capability's irreversible band.
        store.set_open_irreversible("flights", true).unwrap();
        assert_eq!(
            store.opened_overrides().unwrap(),
            vec![("flights".to_owned(), true)]
        );

        // Toggling `enabled` on the same id must NOT reset the opener, and vice
        // versa — the two per-capability flags are independent (ON CONFLICT upsert).
        store.set_enabled("flights", false).unwrap();
        assert_eq!(
            store.opened_overrides().unwrap(),
            vec![("flights".to_owned(), true)],
            "enabling/disabling must not clobber the opener"
        );
        assert_eq!(
            store.enabled_overrides().unwrap(),
            vec![("flights".to_owned(), false)]
        );

        // Re-close it.
        store.set_open_irreversible("flights", false).unwrap();
        assert_eq!(
            store.opened_overrides().unwrap(),
            vec![("flights".to_owned(), false)]
        );
        assert_eq!(
            store.enabled_overrides().unwrap(),
            vec![("flights".to_owned(), false)],
            "re-closing the opener must not clobber enabled"
        );
    }

    #[test]
    fn confirm_round_trips_and_is_independent_of_the_other_flags() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);

        // Off by default: no rows.
        assert!(store.confirm_overrides().unwrap().is_empty());

        // Set "ask first" for a read-only skill.
        store.set_confirm("weather", true).unwrap();
        assert_eq!(
            store.confirm_overrides().unwrap(),
            vec![("weather".to_owned(), true)]
        );

        // enabled / opener / confirm are three independent per-skill flags.
        store.set_enabled("weather", false).unwrap();
        store.set_open_irreversible("weather", true).unwrap();
        assert_eq!(
            store.confirm_overrides().unwrap(),
            vec![("weather".to_owned(), true)],
            "toggling enabled/opener must not clobber confirm"
        );
        assert_eq!(
            store.enabled_overrides().unwrap(),
            vec![("weather".to_owned(), false)]
        );

        // Clear it.
        store.set_confirm("weather", false).unwrap();
        assert_eq!(
            store.confirm_overrides().unwrap(),
            vec![("weather".to_owned(), false)]
        );
    }

    /// A unique file path under the system temp directory, so a store can be closed and
    /// reopened the way a restart would.
    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("endora-{name}-{unique}.db"))
    }

    fn store_at(path: &std::path::Path) -> ConfigStore {
        let db = Db::open(path.to_str().unwrap()).unwrap();
        migrate(&db).unwrap();
        ConfigStore::new(db)
    }

    #[test]
    fn a_change_and_its_undo_survive_a_restart() {
        // The whole point of ADR 0054. ADR 0054 captured the prior value and dropped it
        // on the floor, so the undo existed for the length of one function call.
        use crate::application::{ConfigWrite, ConfigWriteLog};
        let path = temp_db_path("restart");
        let write = ConfigWrite {
            id: 42,
            at_ms: 1_700_000_000_000,
            server: "home-assistant".to_owned(),
            target: "light.kitchen_table".to_owned(),
            added: "table".to_owned(),
            was: vec!["kitchen table light".to_owned()],
            undone: false,
            kind: crate::domain::WriteKind::Name,
        };
        store_at(&path).record(&write).unwrap();
        // A second store over the same file, the way it would be after a restart.
        let found = store_at(&path)
            .write(42)
            .unwrap()
            .expect("the change was not kept");
        assert_eq!(found, write, "the record came back different");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn undoing_marks_the_change_and_never_deletes_it() {
        // What Endora changed about someone's house is not something it should be able to
        // make disappear.
        use crate::application::{ConfigWrite, ConfigWriteLog};
        let path = temp_db_path("undo");
        let store = store_at(&path);
        store
            .record(&ConfigWrite {
                id: 7,
                at_ms: 1,
                server: "home-assistant".to_owned(),
                target: "light.x".to_owned(),
                added: "lamp".to_owned(),
                was: Vec::new(),
                undone: false,
                kind: crate::domain::WriteKind::Name,
            })
            .unwrap();
        store.mark_undone(7).unwrap();
        let found = store.write(7).unwrap().expect("the row was deleted");
        assert!(found.undone, "the change was not marked");
        assert_eq!(store.writes(10).unwrap().len(), 1, "history lost a row");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_name_containing_a_comma_comes_back_whole() {
        // `was` is a list, and flattening it to a delimited string would corrupt any name
        // containing the delimiter — exactly what an undo cannot afford to get wrong.
        use crate::application::{ConfigWrite, ConfigWriteLog};
        let path = temp_db_path("commas");
        let store = store_at(&path);
        let awkward = vec!["lamp, tall".to_owned(), "reading \"light\"".to_owned()];
        store
            .record(&ConfigWrite {
                id: 9,
                at_ms: 1,
                server: "s".to_owned(),
                target: "light.x".to_owned(),
                added: "new".to_owned(),
                was: awkward.clone(),
                undone: false,
                kind: crate::domain::WriteKind::Name,
            })
            .unwrap();
        assert_eq!(store.write(9).unwrap().unwrap().was, awkward);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_collection_is_never_undone_as_if_it_were_a_name() {
        // The hazard the stored kind exists for. A collection is created with no prior
        // value, which reads exactly like adding a name — so undoing it as one would
        // replay an empty list and strip every name off whatever it points at.
        use crate::application::{ConfigWrite, ConfigWriteLog};
        use crate::domain::WriteKind;
        let path = temp_db_path("kinds");
        let store = store_at(&path);
        let collection = ConfigWrite {
            id: 11,
            at_ms: 1,
            server: "home-assistant".to_owned(),
            target: "01JABCDEF".to_owned(),
            added: "All Lights".to_owned(),
            was: vec!["light.kitchen_table".to_owned(), "light.garage".to_owned()],
            undone: false,
            kind: WriteKind::Collection,
        };
        store.record(&collection).unwrap();
        let back = store.write(11).unwrap().expect("the change was not kept");
        assert_eq!(
            back.kind,
            WriteKind::Collection,
            "came back as the wrong sort"
        );
        assert!(
            !back.is_removal(),
            "a collection is not a name being taken away"
        );
        assert!(
            back.describe().contains("stands for"),
            "{}",
            back.describe()
        );
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod standing_trouble_tests {
    use super::*;

    fn store() -> ConfigStore {
        let db = endora_persistence::Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        ConfigStore::new(db)
    }

    fn seen(server: &str, thing: &str, at_ms: i64) -> StandingTrouble {
        StandingTrouble {
            server: server.to_owned(),
            thing: thing.to_owned(),
            trouble: "unavailable".to_owned(),
            since_ms: at_ms,
            accepted: false,
        }
    }

    #[test]
    fn the_clock_starts_when_it_is_first_seen_and_does_not_restart() {
        // Every heartbeat re-reports the same trouble. If a later sighting moved `since_ms`
        // forward, nothing would ever be old enough to be worth mentioning and the whole
        // mechanism would report "since earlier today" forever.
        let store = store();
        store
            .note_trouble(&seen("house", "Living Room Lamp", 1_000))
            .unwrap();
        store
            .note_trouble(&seen("house", "Living Room Lamp", 999_000))
            .unwrap();
        let open = store.troubles().unwrap();
        assert_eq!(open.len(), 1, "{open:?}");
        assert_eq!(open[0].since_ms, 1_000);
    }

    #[test]
    fn a_thing_that_answers_again_leaves_nothing_behind() {
        // The anti-queue guarantee, made structural: the store holds what is wrong *now*,
        // so it is bounded by the state of the house rather than by uptime. A device that
        // recovers is not history to be groomed — it is simply no longer a problem.
        let store = store();
        store
            .note_trouble(&seen("house", "Porch Light", 1_000))
            .unwrap();
        store.accept_trouble("house", "Porch Light").unwrap();
        store.clear_trouble("house", "Porch Light").unwrap();
        assert!(store.troubles().unwrap().is_empty());

        // And if it goes wrong again later it is a fresh problem with a fresh clock,
        // not a resurrected old one that the person already answered.
        store
            .note_trouble(&seen("house", "Porch Light", 500_000))
            .unwrap();
        let open = store.troubles().unwrap();
        assert_eq!(open[0].since_ms, 500_000);
        assert!(
            !open[0].accepted,
            "an old answer must not silence a new fault"
        );
    }

    fn reading(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    const DAY: i64 = 86_400_000;

    #[test]
    fn nothing_is_said_until_it_has_been_wrong_long_enough() {
        // A device that goes quiet for an evening and comes back is not a problem, and a
        // butler that mentions it is the kind you stop reading. The clock has to run first.
        let store = store();
        let seen = reading(&[("Living Room Lamp", "unavailable"), ("Kitchen Main", "on")]);
        crate::application::watch_for_trouble(&store, "house", &seen, 0).unwrap();

        let open = store.troubles().unwrap();
        assert!(crate::application::worth_raising(&open, DAY).is_empty());
        let after_four_days = crate::application::worth_raising(&open, 4 * DAY);
        assert_eq!(after_four_days.len(), 1);
        assert_eq!(
            after_four_days[0].statement(4 * DAY),
            "Living Room Lamp has not answered for 4 days"
        );
    }

    #[test]
    fn a_reading_that_shows_it_well_again_ends_the_matter() {
        // The store tracks the present, not a history. This is the whole anti-queue
        // argument: it cannot grow with uptime, only with what is actually wrong.
        let store = store();
        crate::application::watch_for_trouble(
            &store,
            "house",
            &reading(&[("Porch Light", "unavailable")]),
            0,
        )
        .unwrap();
        assert_eq!(store.troubles().unwrap().len(), 1);

        crate::application::watch_for_trouble(
            &store,
            "house",
            &reading(&[("Porch Light", "off")]),
            5 * DAY,
        )
        .unwrap();
        assert!(store.troubles().unwrap().is_empty());
    }

    #[test]
    fn only_words_that_can_mean_nothing_else_count_as_trouble() {
        // Services differ on the word and there is no protocol question that settles it,
        // so this is a heuristic — one that can only ever produce a question, never an
        // action. A real reading, however unusual, must never be mistaken for absence.
        for absent in ["unavailable", "offline", "UNAVAILABLE", " disconnected "] {
            assert!(crate::domain::not_answering(absent), "{absent:?}");
        }
        for present in ["on", "off", "72", "idle", "0", "closed", "unlocked"] {
            assert!(!crate::domain::not_answering(present), "{present:?}");
        }
        // From the first live reading, which flagged 28 things against 7 real ones. Every
        // false positive was a scene, whose state is when it was last activated —
        // `unknown` means "not since the restart", the healthiest answer available.
        for healthy in ["unknown", "none", "null", "error", "", "  "] {
            assert!(
                !crate::domain::not_answering(healthy),
                "a word that also means 'hasn't happened yet' is not trouble: {healthy:?}"
            );
        }
    }

    #[test]
    fn accepting_one_keeps_it_without_raising_it() {
        let store = store();
        store
            .note_trouble(&seen("house", "Shed Sensor", 1_000))
            .unwrap();
        store
            .note_trouble(&seen("house", "Attic Fan", 2_000))
            .unwrap();
        store.accept_trouble("house", "Shed Sensor").unwrap();
        let open = store.troubles().unwrap();
        assert_eq!(open.len(), 2, "accepting is not forgetting: {open:?}");
        assert!(
            open.iter()
                .find(|t| t.thing == "Shed Sensor")
                .unwrap()
                .accepted
        );
        assert!(
            !open
                .iter()
                .find(|t| t.thing == "Attic Fan")
                .unwrap()
                .accepted
        );
    }
}
