//! Endora CLI binary.
//!
//! The CLI is a thin, replaceable client — one of several possible interfaces
//! to the authoritative node. Like every client it holds no authority of its
//! own; it will speak the versioned protocol to the node. In the foundation
//! phase it only identifies itself and exits successfully.

#![forbid(unsafe_code)]

fn main() {
    println!("{}", endora_application::platform_identity());
    println!("cli: thin client (foundation phase — no commands yet)");
}
