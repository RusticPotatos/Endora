//! Capabilities persistence — SQLite adapters for skill config, settings, the
//! autonomy envelope, and the deep-model configuration, over the shared handle.

use endora_kernel::RepositoryError;
use endora_persistence::{Db, backend};
use rusqlite::{OptionalExtension, params};

use crate::application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilitySettingsRepository, DeepModel, DeepModelRepository,
    ModelSlot, Sampling,
};

/// Creates the capabilities config tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_config (
                id      TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL
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
            ) STRICT;",
        )
        .map_err(backend)?;
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
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO capability_config (id, enabled) VALUES (?1, ?2)",
                params![id, i64::from(enabled)],
            )
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
}
