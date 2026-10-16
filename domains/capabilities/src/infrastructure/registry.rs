use super::*;

// ---- Application-facing runner ---------------------------------------------

/// Adapts the concrete capability registry to the application's
/// [`CapabilityRunner`] port, so the butler use case can list and run skills
/// without depending on this crate. A capability is "autonomous" (may run on its
/// own this turn) exactly when the classifier's verdict for its [`Reversibility`]
/// band and the person's envelope is [`Act`](Decision::Act). Anything that must
/// confirm — or is blocked — stays gated.
pub struct RegistryRunner {
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// The person's per-tool stance (ADR 0062). Missing = the band's default.
    stances: std::collections::HashMap<String, Stance>,
    /// Tools the record has proven — enough read-back confirmed changes (ADR 0062).
    /// Derived from outcomes at composition, never stored.
    proven: std::collections::HashSet<String>,
    /// The person's autonomy envelope — the boundary the butler acts within.
    envelope: crate::application::AutonomyEnvelope,
    /// Per-capability settings (id → key/value), for skills that need config.
    settings: std::collections::HashMap<String, CapabilitySettings>,
}

impl RegistryRunner {
    /// Wraps a shared capability registry at its defaults: no stances stored, nothing
    /// proven, the default envelope, no settings.
    #[must_use]
    pub fn new(capabilities: Arc<Vec<Arc<dyn Capability>>>) -> Self {
        Self {
            capabilities,
            stances: std::collections::HashMap::new(),
            proven: std::collections::HashSet::new(),
            envelope: crate::application::AutonomyEnvelope::default(),
            settings: std::collections::HashMap::new(),
        }
    }

    /// Wraps the registry with the person's stances (ADR 0062), what the record has
    /// proven, their envelope (ADR 0051), and per-capability settings (ADR 0054).
    #[must_use]
    pub fn with_config(
        capabilities: Arc<Vec<Arc<dyn Capability>>>,
        stances: Vec<(String, Stance)>,
        proven: std::collections::HashSet<String>,
        envelope: crate::application::AutonomyEnvelope,
        settings: std::collections::HashMap<String, CapabilitySettings>,
    ) -> Self {
        Self {
            capabilities,
            stances: stances.into_iter().collect(),
            proven,
            envelope,
            settings,
        }
    }

    /// The person's stance on a tool, or its band's default (ADR 0062).
    fn stance_of(&self, info: &CapabilityInfo) -> Stance {
        self.stances
            .get(info.id)
            .copied()
            .unwrap_or_else(|| default_stance(info.reversibility))
    }

    /// Whether a capability may appear at all — `off` is visible but never offered.
    fn is_enabled(&self, info: &CapabilityInfo) -> bool {
        self.stance_of(info) != Stance::Off
    }

    /// The stored settings for a capability (empty if none set).
    fn settings_for(&self, id: &str) -> CapabilitySettings {
        // A skill that rides another's connection reads that one's settings (ADR 0058:
        // one credential, one place). Declared by the skill, so nothing here knows an
        // integration by name.
        let borrowed = self
            .capabilities
            .iter()
            .map(|c| c.info())
            .find(|i| i.id == id)
            .and_then(|i| i.borrows_from)
            .and_then(|from| self.settings.get(from).cloned());
        borrowed
            .or_else(|| self.settings.get(id).cloned())
            .unwrap_or_default()
    }
}

/// Whether a capability's settings let it run: every required one filled, or, for one that
/// rides another's connection (`borrows_from`), that connection's settings present. `settings`
/// is what [`RegistryRunner`] resolves for it: the connection's, for a borrower. The one rule
/// for chat and the settings screen, which once disagreed: the screen said a borrowing skill
/// was on before its connection was set up.
#[must_use]
pub fn settings_ready(info: &CapabilityInfo, settings: &CapabilitySettings) -> bool {
    match info.borrows_from {
        Some(_) => !settings.is_empty(),
        None => settings_complete(info, settings),
    }
}

/// The band's default stance, where the person has said nothing (ADR 0062).
///
/// A read reports the world and runs; something reversible asks; the un-undoable — and
/// every unproven MCP tool, which is classed with it because a server's self-report is not
/// evidence — is blocked until somebody moves it. Deny-by-default lives here.
#[must_use]
pub const fn default_stance(band: Reversibility) -> Stance {
    match band {
        Reversibility::Observe => Stance::Auto,
        // Reversible on-device runs; outward-but-reversible asks, because reach is the
        // person's second dial and `auto` would take it out of their hands by default.
        Reversibility::Reversible => Stance::Auto,
        Reversibility::OutwardReversible => Stance::Ask,
        Reversibility::Irreversible => Stance::Off,
    }
}

/// Whether every setting a capability declares has a value — i.e. it is set up.
/// Whether every setting a skill actually needs has a value.
///
/// Public because the interface asks the same question, and had been answering it with its
/// own copy — which then did not learn that settings can be optional, so a skill stayed
/// "needs setup" in the console after policy had already decided it was ready. One rule,
/// one implementation.
#[must_use]
pub fn settings_complete(info: &CapabilityInfo, settings: &CapabilitySettings) -> bool {
    info.settings
        .iter()
        .filter(|s| !s.optional)
        .all(|s| settings.get(s.key).is_some_and(|v| !v.trim().is_empty()))
}

/// The deterministic classifier (ADR 0051's boundary, ADR 0062's mechanism): one stance,
/// the band, the record, and the envelope — never the model.
///
/// The whole ladder:
///
/// - **`off`** blocks, whoever set it and whyever.
/// - **`ask`** confirms each use — and **graduates**: a tool the record has proven (enough
///   read-back confirmed changes, counted in code) acts on its own while the envelope
///   allows consequential actions. Narrow the envelope and every graduate asks again.
/// - **`auto`** acts — narrowed to confirm for an external read when the person has kept
///   off-device actions in hand, exactly as before.
///
/// What graduation can never do: move `off` (a stance the person set is a decision, and the
/// record does not overrule decisions), or lift a tool nobody vetted (an unproven
/// irreversible-band tool has no `ask` to graduate from — its default is `off`).
pub(super) fn classify(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    stance: Stance,
    proven: bool,
) -> Decision {
    match stance {
        Stance::Off => Decision::Block,
        Stance::Ask => {
            if proven && env.auto_consequential {
                Decision::Act
            } else {
                Decision::Confirm
            }
        }
        Stance::Auto => {
            if info.reaches_external && !env.auto_external {
                Decision::Confirm
            } else {
                Decision::Act
            }
        }
    }
}

/// Whether a skill may run on its own this turn — exactly when the deterministic
/// [`classify`] verdict is [`Act`](Decision::Act).
pub(super) fn may_run_autonomously(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    stance: Stance,
    proven: bool,
) -> bool {
    classify(info, env, stance, proven) == Decision::Act
}

impl CapabilityRunner for RegistryRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.capabilities
            .iter()
            .map(|c| {
                let info = c.info();
                crate::application::CapabilitySpec {
                    id: info.id.to_owned(),
                    // The declaration, carried — not hardcoded false, which is what
                    // shipped: every built-in lost both flags at this seam, so the
                    // place guard (ADR 0065) never fired for weather/news and the
                    // stranger taint (ADR 0064) never fired for news/mail. Live, a
                    // model-invented city sailed straight into a weather call the
                    // guard existed to catch, and the butler talked about Springfield
                    // to a person who never said it.
                    wants_place: info.wants_place,
                    third_party: info.third_party,
                    description: info.description.to_owned(),
                    // Usable only if the code is ready, the person has it enabled, AND
                    // every required setting has a value (ADR 0054).
                    configured: info.configured
                        && self.is_enabled(&info)
                        && settings_ready(&info, &self.settings_for(info.id)),
                    reversibility: info.reversibility,
                    autonomous: may_run_autonomously(
                        &info,
                        &self.envelope,
                        self.stance_of(&info),
                        self.proven.contains(info.id),
                    ),
                    // Built-ins describe their inputs in the prompt, not a schema.
                    input_schema: info.input_schema.map(str::to_owned),
                }
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.capabilities
            .iter()
            .find(|c| c.info().id == id)
            .map(|c| {
                let info = c.info();
                classify(
                    &info,
                    &self.envelope,
                    self.stance_of(&info),
                    self.proven.contains(id),
                )
            })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        self.run_observed(id, input_json).map(|read| read.text)
    }

    fn run_observed(
        &self,
        id: &str,
        input_json: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        let cap = self
            .capabilities
            .iter()
            .find(|c| c.info().id == id)
            .ok_or_else(|| format!("no such skill '{id}'"))?;

        // Deny-by-default on the irreversible band (ADR 0051): the un-undoable is
        // blocked outright — never run, even on an explicit request — until the
        // person opens it per capability. Once opened it reaches this path only via
        // an explicit confirmation. The failure mode is "it refused," never "it did
        // something permanent." The classifier owns which band is blocked.
        let info = cap.info();
        if classify(
            &info,
            &self.envelope,
            self.stance_of(&info),
            self.proven.contains(id),
        ) == Decision::Block
        {
            // One state (`off`), worded by why it is off: the un-undoable was never
            // allowed, anything else the person turned off (ADR 0062).
            return Err(if info.reversibility == Reversibility::Irreversible {
                format!(
                    "the '{id}' skill can't be undone, so Endora won't run it on its own — \
                     it stays blocked until you allow it for this skill"
                )
            } else {
                format!("the '{id}' skill is turned off")
            });
        }
        // Data-loss tripwire: for a skill that leaves the device, refuse to send a
        // request that appears to carry a secret (ADR 0051). Fail closed.
        if cap.info().reaches_external {
            if let Some(kind) = scan_outbound_secret(input_json) {
                return Err(format!(
                    "refusing to send this to '{id}' — the request looks like it contains {kind}"
                ));
            }
        }
        let mut input: Value = serde_json::from_str(input_json.trim())
            .or_else(|_| Ok::<Value, serde_json::Error>(json!({})))
            .unwrap_or_else(|_| json!({}));
        // Query minimization: strip personal identifiers (email addresses) from a
        // request before it leaves the device (ADR 0051).
        if cap.info().reaches_external {
            redact_pii_in_value(&mut input);
        }
        let out = cap
            .invoke(&input, &self.settings_for(id))
            .map_err(|e| e.to_string())?;
        // Hand the butler readable text, not raw JSON — small models relay it far
        // more reliably (and won't miss headlines buried in a JSON array).
        Ok(crate::application::CapabilityRead {
            text: cap.summarize(&out),
            data: Some(out.to_string()),
            input: input.to_string(),
        })
    }
}
