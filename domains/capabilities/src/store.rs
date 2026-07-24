//! Capabilities persistence — SQLite adapters for skill config, settings, the
//! autonomy envelope, and the deep-model configuration, over the shared handle.

use endora_kernel::RepositoryError;
use endora_persistence::{Db, backend, corrupt};
use rusqlite::{OptionalExtension, params};

use crate::application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilitySettingsRepository, DeepModel, DeepModelRepository,
    McpServer, McpServerRegistry, McpTransport, ModelSlot, ModelTuneSchedule,
    ModelTuneScheduleRepository, Sampling,
};

/// Creates the capabilities config tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
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
                id      INTEGER PRIMARY KEY CHECK (id = 0),
                url     TEXT NOT NULL,
                model   TEXT NOT NULL,
                api_key TEXT NOT NULL
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
            CREATE TABLE IF NOT EXISTS mcp_servers (
                name    TEXT PRIMARY KEY,
                kind    TEXT NOT NULL,
                command TEXT NOT NULL DEFAULT '',
                args    TEXT NOT NULL DEFAULT '',
                url     TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                env     TEXT NOT NULL DEFAULT '',
                auth    TEXT NOT NULL DEFAULT '',
                trust_all INTEGER NOT NULL DEFAULT 1
            ) STRICT;",
        )
        .map_err(backend)?;
    // Additive migrations for databases created before these columns existed —
    // the irreversible-opener (ADR 0024) and the ask-first override. Ignore the
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
        "ALTER TABLE mcp_servers ADD COLUMN trust_all INTEGER NOT NULL DEFAULT 1",
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
                "SELECT url, model, api_key FROM deep_model WHERE id = 0",
                [],
                |r| {
                    Ok(DeepModel {
                        url: r.get(0)?,
                        model: r.get(1)?,
                        api_key: r.get(2)?,
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
                "INSERT OR REPLACE INTO deep_model (id, url, model, api_key) VALUES (0, ?1, ?2, ?3)",
                params![model.url, model.model, model.api_key],
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

impl McpServerRegistry for ConfigStore {
    fn list(&self) -> Result<Vec<McpServer>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT name, kind, command, args, url, enabled, env, auth, trust_all \
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
                ))
            })
            .map_err(backend)?;
        let mut servers = Vec::new();
        for row in rows {
            let (name, kind, command, args_json, url, enabled, env_json, auth, trust_all) =
                row.map_err(backend)?;
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
                 (name, kind, command, args, url, enabled, env, auth, trust_all) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
    fn mcp_trust_all_defaults_on_round_trips_and_toggles() {
        use crate::application::{McpServer, McpServerRegistry, McpTransport};

        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ConfigStore::new(db);

        // A freshly constructed server auto-allows its tools by default.
        let ha = McpServer::http("home-assistant", "https://ha.local/mcp_server/sse").unwrap();
        assert!(ha.trust_all);
        store.register(&ha).unwrap();
        assert!(store.list().unwrap()[0].trust_all);

        // Turning it off persists and leaves the transport untouched.
        McpServerRegistry::set_trust_all(&store, "home-assistant", false).unwrap();
        let row = store.list().unwrap().into_iter().next().unwrap();
        assert!(!row.trust_all);
        assert_eq!(
            row.transport,
            McpTransport::Http {
                url: "https://ha.local/mcp_server/sse".to_owned(),
                auth: String::new(),
            }
        );

        // And back on.
        McpServerRegistry::set_trust_all(&store, "home-assistant", true).unwrap();
        assert!(store.list().unwrap()[0].trust_all);
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
}
