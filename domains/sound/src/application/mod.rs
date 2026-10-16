//! What the hub does with the patchbay: every change applied whole, kept, then made live.

pub mod engine;
pub mod hub;
pub mod ports;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod hub_tests;
