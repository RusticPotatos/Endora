//! Models in ordered lists (ADR 0105): providers entered once, and for each job (everyday,
//! deep) a sorted list of a provider and a model. The first entry that works, and is cleared
//! for the request, answers; the walking is infrastructure's, the shape is here.

use crate::application::{ButlerModelConfig, DeepModel};
use endora_kernel::RepositoryError;

/// An endpoint and its key, entered once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Its id.
    pub id: i64,
    /// What the owner calls it: "Anthropic", "The Mac".
    pub name: String,
    /// Its OpenAI-compatible base URL (`.../v1`).
    pub url: String,
    /// Its key, sent as a bearer token; empty for a keyless local endpoint. Never returned to a
    /// browser.
    pub api_key: String,
}

/// What a list is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Job {
    /// The conversation.
    Everyday,
    /// A bigger model, asked on purpose or when the everyday one cannot answer.
    Deep,
}

impl Job {
    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Everyday => "everyday",
            Self::Deep => "deep",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Everyday, Self::Deep]
            .into_iter()
            .find(|j| j.name() == name)
    }
}

/// One place in a list: a provider and a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its id.
    pub id: i64,
    /// Which list.
    pub job: Job,
    /// Which provider.
    pub provider: i64,
    /// Which model there.
    pub model: String,
    /// Switched off: kept, but skipped.
    pub enabled: bool,
}

/// What one enabled entry resolves to: where to send, what to ask for, with which key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The entry.
    pub entry: i64,
    /// The provider's name.
    pub provider: String,
    /// The endpoint.
    pub url: String,
    /// The model.
    pub model: String,
    /// The key.
    pub api_key: String,
}

/// A list's enabled entries, in order, resolved against their providers. An entry whose
/// provider is gone, or whose model is blank, is skipped.
#[must_use]
pub fn targets(providers: &[Provider], entries: &[Entry]) -> Vec<Target> {
    entries
        .iter()
        .filter(|e| e.enabled && !e.model.trim().is_empty())
        .filter_map(|e| {
            let p = providers.iter().find(|p| p.id == e.provider)?;
            (!p.url.trim().is_empty()).then(|| Target {
                entry: e.id,
                provider: p.name.clone(),
                url: p.url.trim().trim_end_matches('/').to_owned(),
                model: e.model.trim().to_owned(),
                api_key: p.api_key.clone(),
            })
        })
        .collect()
}

/// A name for a provider, from its address, for the ones moved over from the old settings.
#[must_use]
pub fn name_for(url: &str) -> String {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("")
        .to_owned();
    let known = [
        ("api.anthropic.com", "Anthropic"),
        ("api.deepseek.com", "DeepSeek"),
        ("api.openai.com", "OpenAI"),
        ("openrouter.ai", "OpenRouter"),
        ("api.groq.com", "Groq"),
        ("host.docker.internal:11434", "Ollama on the NAS"),
        ("localhost:11434", "Ollama"),
        ("127.0.0.1:11434", "Ollama"),
    ];
    known
        .iter()
        .find(|(h, _)| host == *h || host.ends_with(&format!(".{h}")))
        .map_or(host, |(_, name)| (*name).to_owned())
}

/// What moving the old settings over adds, in order: providers (deduplicated by address and
/// key) and the entries of each list. The preferred machine comes first in everyday, then the
/// stored model (the writer of a mixture, which is what spoke); the deep model is deep's one
/// entry. Nothing is added for a slot that was not set.
#[must_use]
pub fn moved_over(
    everyday: Option<&ButlerModelConfig>,
    deep: Option<&DeepModel>,
) -> Vec<(Job, String, String, String)> {
    let mut out: Vec<(Job, String, String, String)> = Vec::new();
    let mut add = |job: Job, url: &str, key: &str, model: &str| {
        if url.trim().is_empty() || model.trim().is_empty() {
            return;
        }
        out.push((
            job,
            url.trim().to_owned(),
            key.to_owned(),
            model.trim().to_owned(),
        ));
    };
    if let Some(c) = everyday {
        add(
            Job::Everyday,
            &c.preferred_url,
            &c.api_key,
            &c.preferred_model,
        );
        let model = if c.mixture && !c.synth.model.trim().is_empty() {
            &c.synth.model
        } else {
            &c.single.model
        };
        add(Job::Everyday, &c.base_url, &c.api_key, model);
    }
    if let Some(d) = deep {
        add(Job::Deep, &d.url, &d.api_key, &d.model);
    }
    out
}

/// Where the providers and the lists are kept.
pub trait ModelListRepository {
    /// Every provider.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn providers(&self) -> Result<Vec<Provider>, RepositoryError>;

    /// Adds a provider; returns its id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn add_provider(&self, name: &str, url: &str, api_key: &str) -> Result<i64, RepositoryError>;

    /// Changes a provider; its key only when one is given.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn update_provider(
        &self,
        id: i64,
        name: &str,
        url: &str,
        api_key: Option<&str>,
    ) -> Result<(), RepositoryError>;

    /// Removes a provider and every entry that uses it.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn remove_provider(&self, id: i64) -> Result<(), RepositoryError>;

    /// A list's entries, in order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn entries(&self, job: Job) -> Result<Vec<Entry>, RepositoryError>;

    /// Adds an entry at the end of a list; returns its id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn add_entry(&self, job: Job, provider: i64, model: &str) -> Result<i64, RepositoryError>;

    /// Switches an entry on or off.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn enable_entry(&self, id: i64, enabled: bool) -> Result<(), RepositoryError>;

    /// Moves an entry one place up (towards first) or down.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn move_entry(&self, id: i64, up: bool) -> Result<(), RepositoryError>;

    /// Removes an entry.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn remove_entry(&self, id: i64) -> Result<(), RepositoryError>;

    /// Moves the old settings over, once: on the first start with lists, and never again,
    /// so emptying a list is not undone.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn move_over_once(
        &self,
        everyday: Option<&ButlerModelConfig>,
        deep: Option<&DeepModel>,
    ) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{ModelSlot, Sampling};

    fn slot(model: &str) -> ModelSlot {
        ModelSlot {
            model: model.to_owned(),
            sampling: Sampling::default(),
        }
    }

    #[test]
    fn the_old_settings_move_over_in_the_order_they_were_used() {
        let everyday = ButlerModelConfig {
            base_url: "http://host.docker.internal:11434/v1".into(),
            api_key: String::new(),
            mixture: false,
            single: slot("qwen2.5:7b"),
            router: slot(""),
            synth: slot(""),
            preferred_url: "http://192.168.1.10:11434/v1".into(),
            preferred_model: "qwen2.5:32b".into(),
        };
        let deep = DeepModel {
            url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            api_key: "k".into(),
            escalate: true,
        };
        let moved = moved_over(Some(&everyday), Some(&deep));
        let shape: Vec<_> = moved.iter().map(|(j, _, _, m)| (*j, m.as_str())).collect();
        assert_eq!(
            shape,
            [
                (Job::Everyday, "qwen2.5:32b"),
                (Job::Everyday, "qwen2.5:7b"),
                (Job::Deep, "deepseek-chat")
            ]
        );
        assert_eq!(name_for(&moved[2].1), "DeepSeek");
        assert_eq!(name_for(&moved[1].1), "Ollama on the NAS");
        assert_eq!(
            name_for("http://192.168.1.10:11434/v1"),
            "192.168.1.10:11434"
        );
        assert!(moved_over(None, None).is_empty());
    }

    fn store() -> crate::store::ConfigStore {
        let db = endora_persistence::Db::open_in_memory().unwrap();
        crate::store::migrate(&db).unwrap();
        crate::store::ConfigStore::new(db)
    }

    #[test]
    fn moving_over_happens_once_shares_a_provider_and_keeps_its_key() {
        let s = store();
        let local = "http://host.docker.internal:11434/v1";
        let everyday = ButlerModelConfig {
            base_url: local.into(),
            api_key: String::new(),
            mixture: false,
            single: slot("qwen2.5:7b"),
            router: slot(""),
            synth: slot(""),
            preferred_url: local.into(),
            preferred_model: "qwen2.5:14b".into(),
        };
        let deep = DeepModel {
            url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            api_key: "secret".into(),
            escalate: true,
        };
        assert!(s.move_over_once(Some(&everyday), Some(&deep)).unwrap());
        assert!(
            !s.move_over_once(Some(&everyday), Some(&deep)).unwrap(),
            "once only"
        );
        let providers = s.providers().unwrap();
        assert_eq!(
            providers.len(),
            2,
            "one Ollama for both everyday entries, and DeepSeek"
        );
        assert_eq!(providers[1].api_key, "secret");
        let everyday = s.entries(Job::Everyday).unwrap();
        assert_eq!(
            everyday
                .iter()
                .map(|e| e.model.as_str())
                .collect::<Vec<_>>(),
            ["qwen2.5:14b", "qwen2.5:7b"]
        );
        // Emptied by the owner, it is not filled again.
        for e in &everyday {
            s.remove_entry(e.id).unwrap();
        }
        s.move_over_once(None, None).unwrap();
        assert!(s.entries(Job::Everyday).unwrap().is_empty());
    }

    #[test]
    fn entries_sort_switch_off_and_go_with_their_provider() {
        let s = store();
        let claude = s
            .add_provider("Anthropic", "https://api.anthropic.com/v1", "k")
            .unwrap();
        let local = s
            .add_provider("Ollama", "http://localhost:11434/v1", "")
            .unwrap();
        let a = s.add_entry(Job::Everyday, local, "qwen2.5:7b").unwrap();
        let b = s
            .add_entry(Job::Everyday, claude, "claude-sonnet-5-5")
            .unwrap();
        s.add_entry(Job::Deep, claude, "claude-opus-5-5").unwrap();
        s.move_entry(b, true).unwrap();
        s.move_entry(b, true).unwrap(); // already first: nothing
        let ids: Vec<i64> = s
            .entries(Job::Everyday)
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, [b, a]);
        s.enable_entry(a, false).unwrap();
        let resolved = targets(&s.providers().unwrap(), &s.entries(Job::Everyday).unwrap());
        assert_eq!(resolved.len(), 1);
        s.update_provider(claude, "Claude", "https://api.anthropic.com/v1", None)
            .unwrap();
        assert_eq!(
            s.providers().unwrap()[0].api_key,
            "k",
            "a key is kept unless a new one is given"
        );
        s.remove_provider(claude).unwrap();
        assert!(s.entries(Job::Deep).unwrap().is_empty());
        assert_eq!(s.entries(Job::Everyday).unwrap().len(), 1);
    }

    #[test]
    fn a_list_resolves_to_its_enabled_entries_in_order() {
        let providers = [
            Provider {
                id: 1,
                name: "Anthropic".into(),
                url: "https://api.anthropic.com/v1/".into(),
                api_key: "a".into(),
            },
            Provider {
                id: 2,
                name: "Local".into(),
                url: "http://localhost:11434/v1".into(),
                api_key: String::new(),
            },
        ];
        let entry = |id, provider, model: &str, enabled| Entry {
            id,
            job: Job::Everyday,
            provider,
            model: model.into(),
            enabled,
        };
        let list = [
            entry(10, 1, "claude-sonnet-5-5", true),
            entry(11, 3, "gone", true),
            entry(12, 2, "qwen2.5:7b", false),
            entry(13, 2, "qwen2.5:7b", true),
        ];
        let resolved = targets(&providers, &list);
        let shape: Vec<_> = resolved.iter().map(|t| (t.entry, t.url.as_str())).collect();
        assert_eq!(
            shape,
            [
                (10, "https://api.anthropic.com/v1"),
                (13, "http://localhost:11434/v1")
            ]
        );
    }
}
