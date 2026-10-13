/// A server's **own** interface, richer than the tool surface it exposes to a model
/// (ADR 0054).
///
/// A tool catalogue is a product decision by whoever wrote the server, and it is often
/// the *voice assistant* view: fuzzy names, no identifiers. The same service usually has
/// an API underneath where things have ids and cannot be mistaken for one another. Where
/// Endora is given that reach, it should use it — and nothing above this port learns
/// which service it is talking to.
///
/// Deliberately three methods. This is not "run arbitrary API calls": it is *see what
/// exists*, and *act on exactly one of them*.
pub trait NativeChannel: Send + Sync {
    /// Everything the server knows about, as `(id, name)` — the unambiguous identifier
    /// and the name a person would say.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn known(&self) -> Result<Vec<(String, String)>, String>;

    /// The same knowledge rendered as text, so the target search reads it exactly as it
    /// reads any other reading.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn reading(&self) -> Result<String, String>;

    /// Does what `tool` was trying to do, to exactly one thing, by id. `None` when this
    /// channel cannot express that particular tool — the caller falls back.
    fn act(&self, tool: &str, id: &str) -> Option<Result<String, String>>;

    /// Whether `tool` could actually operate this thing (ADR 0054).
    ///
    /// Direct reach sees everything a service holds, which includes things that are not
    /// controls: diagnostics, configuration entries, connection indicators. Those share
    /// their device's name almost exactly, so they tie with it in a search and turn a
    /// clear request into an ambiguous one.
    ///
    /// True by default — a channel that cannot tell says so by not narrowing anything.
    fn actionable(&self, tool: &str, id: &str) -> bool {
        let _ = (tool, id);
        true
    }

    /// The things this service says belong to **the person**, rather than to the household.
    ///
    /// Decides whether a reading may ever become a belief about them (ADR 0057). Empty is the
    /// honest default and the safe direction: nothing is attributed, rather than the house
    /// being mistaken for the person.
    fn belongs_to_the_person(&self) -> Vec<String> {
        Vec::new()
    }

    /// What this service already has set up, by whatever name it calls each thing.
    ///
    /// The Connect screen offered every service with a **Connect** button and knew nothing
    /// about what was already there, so somebody with a calendar working saw "Connect" beside
    /// it and had nowhere to find out whether it had worked. Offering to do something already
    /// done is worse than not offering — it reads as though the last attempt failed.
    ///
    /// Empty is the honest default: a channel that cannot tell says nothing rather than
    /// implying nothing is connected.
    fn already_connected(&self) -> Vec<String> {
        Vec::new()
    }

    /// Begins connecting a new kind of thing to this service, returning **the service's own
    /// setup form** (ADR 0054).
    ///
    /// Endora does not know what a calendar or a mail account needs, and deliberately does
    /// not learn: the service declares its fields and Endora renders them. A kind of thing
    /// nobody here has heard of works the same as one that ships today.
    ///
    /// `None` from a channel with no notion of setting things up.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn begin_setup(&self, _kind: &str) -> Option<Result<crate::domain::SetupForm, String>> {
        None
    }

    /// Answers a form from [`begin_setup`](Self::begin_setup).
    ///
    /// Returns the next form when the service wants more, and `None` inside the `Ok` when it
    /// is finished. **Nothing here is stored.** A credential travels from the person's
    /// keyboard to their own service and is not written down on the way — Endora is passing
    /// a message, not keeping an account.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn finish_setup(
        &self,
        _form: &str,
        _answers: &[(String, String)],
    ) -> Option<Result<Option<crate::domain::SetupForm>, String>> {
        None
    }

    /// Reaches the person when they are not looking at Endora (ADR 0056).
    ///
    /// **Nominated, never assumed.** The same shape as naming a server's reader: the person
    /// says which of their own services is how they want to be reached, and Endora uses it.
    /// `None` by default — from a channel that cannot, and from one that can but has not
    /// been told to. Being able to interrupt somebody is a grant, not a capability that
    /// switches itself on.
    ///
    /// This is deliberately **not** a notification feature built into Endora. A push stack
    /// of its own would mean certificates, a service worker, a subscription store and a
    /// third-party relay — all to duplicate something the person already has working on
    /// their phone. Endora does not host the model ([0055](../../docs/adr/0055-the-model-layer.md))
    /// and by the same reasoning it does not host a push service.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn notify(&self, _title: &str, _body: &str) -> Option<Result<(), String>> {
        None
    }

    /// Takes something out of the service's own view, or puts it back (ADR 0056).
    ///
    /// The remedy for a thing that has not answered in days and that the person has
    /// confirmed is gone. **Hidden, never deleted**: deleting is destructive, irreversible
    /// from Endora's side, and presumes an opinion about somebody else's house that a
    /// tapped answer does not license. Hiding is the smallest change that makes the
    /// catalogue true again, and it undoes exactly.
    ///
    /// `None` from a channel that cannot do it, which is the default.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn hide(
        &self,
        _name: &str,
        _hidden: bool,
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        None
    }

    /// Teaches the service that `alias` is another name for the thing it currently calls
    /// `name`, so the service itself resolves it from then on — for every client, not
    /// only for Endora (ADR 0054).
    ///
    /// `None` from a channel that cannot be taught, which is the default: seeing and
    /// acting are a much smaller grant than editing, and a channel earns the third
    /// separately from the first two.
    ///
    /// Returns the **write**, not a sentence — carrying what was changed and what it was
    /// before, so the edit can be logged and put back (ADR 0054). A channel that reports
    /// only that it succeeded has made a change nobody can reverse.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn teach(&self, name: &str, alias: &str) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, alias);
        None
    }

    /// What this service says is true right now, as `(name, state)`.
    ///
    /// The facts an answer about state should agree with. A service that cannot be asked
    /// says nothing, and nothing downstream changes.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn states(&self) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }

    /// What this service can say about the person **right now**, in one short line.
    ///
    /// `None` — the default — from a service that knows nothing about them. A smart home
    /// knows whether they are in it; a calendar would know whether they are busy.
    fn about_the_person(&self) -> Option<String> {
        None
    }

    /// A reason this call cannot do anything, so it is refused rather than sent
    /// (ADR 0054).
    ///
    /// `None` — the default — means the channel has nothing to say and the call goes out.
    ///
    /// The point is not politeness. A call that quietly does nothing is the worst kind of
    /// failure: it reports success, changes nothing, and leaves the person and the record
    /// disagreeing. Turning it into an outright refusal is also what makes it derivable as
    /// a finding later.
    fn refuse(&self, tool: &str, input_json: &str) -> Option<String> {
        let _ = (tool, input_json);
        None
    }

    /// Narrows a call that already names exactly one thing, before it is sent
    /// (ADR 0054).
    ///
    /// `None` when there is nothing to narrow, which is the default and the common case.
    ///
    /// This is the one place the channel acts **before** a failure rather than after, and
    /// it is allowed for a single reason: it can only ever make a call hit *less*. Live,
    /// a request for one light arrived as
    /// `{entity_id: "light.kitchen_table", area: "kitchen"}` — an exact identifier and a
    /// whole room. The service matched the room, switched off both kitchen lights, and
    /// reported success, so every guard that watches the failure path stayed silent.
    fn tighten(&self, input_json: &str) -> Option<String> {
        let _ = input_json;
        None
    }

    /// The words this service uses as **categories** — the sorts of thing it has, as
    /// opposed to the names of particular things (ADR 0054).
    ///
    /// Empty by default and empty for any service Endora cannot ask, in which case
    /// nothing downstream changes: guessing at what counts as a category would make an
    /// ordinary `domain: ["light"]` pollute every search.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn categories(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    /// Makes **one thing that stands for many** — so a request meaning "all of them" can
    /// be an ordinary single-target action (ADR 0054).
    ///
    /// This is the general answer to a request no amount of aiming can express. "Turn off
    /// all the lights" produces a call aimed at nothing, which is indistinguishable from a
    /// model that failed to say what it meant — so it is refused, correctly, and the
    /// person cannot have the thing they asked for.
    ///
    /// Rather than teaching Endora to fan out across many ids at action time — the one
    /// move every guard here exists to prevent — the service is asked to hold a collection
    /// once. After that there is nothing special about it: it is a thing with a name and
    /// an id, hit exactly like any other.
    ///
    /// `None` from a service that cannot hold one, which is the default.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn collect(
        &self,
        name: &str,
        ids: &[String],
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, ids);
        None
    }

    /// Takes a name away again — the other half of [`teach`](Self::teach), so a name can
    /// be untold and not only added (ADR 0054).
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn forget(
        &self,
        name: &str,
        alias: &str,
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, alias);
        None
    }

    /// Puts a change back exactly as it was, from the record of it.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn undo(&self, write: &crate::domain::ConfigWrite) -> Option<Result<String, String>> {
        let _ = write;
        None
    }
}
