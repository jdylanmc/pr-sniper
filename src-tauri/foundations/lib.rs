//! Native persistence/policy proof while the complete host still needs adapters.
//! These are production source files, not copies or alternate implementations.
#[cfg(test)]
mod copilot;
pub mod github;
#[path = "../src/policy.rs"]
pub mod policy;
#[path = "../src/process_path.rs"]
pub mod process_path;
#[path = "../src/seeds.rs"]
mod seeds;
#[cfg(windows)]
#[path = "../src/startup.rs"]
pub mod startup;
#[path = "../src/storage.rs"]
pub mod storage;
