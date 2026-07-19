//! Endora node binary.
//!
//! The node is Endora's authoritative backend runtime — the "brain". Clients
//! are replaceable and talk to it through a stable, versioned protocol. In the
//! foundation phase this binary only identifies itself and exits successfully;
//! it deliberately does not stand up a fake API or service architecture. The
//! HTTP/JSON protocol, persistence, and policy engine arrive with the first
//! real vertical slice (see `docs/architecture.md` and `docs/adr/`).

#![forbid(unsafe_code)]

fn main() {
    println!("{}", endora_application::platform_identity());
    println!(
        "node: authoritative runtime (foundation phase — no protocol surface yet). \
         default autonomy: {:?}",
        endora_application::default_autonomy_level()
    );
}
