//! What the hub does with the patchbay: every change applied whole, kept, then made live.

pub mod hub;
pub mod ports;

#[cfg(test)]
mod hub_tests;
