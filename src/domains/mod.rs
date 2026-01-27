//! Domain extension hook (stub).
//! Reserved for future per-agent context profiles. No implementation in v0.1.0 —
//! the multi-domain plugin abstraction was removed.
//! §1.4 + §4.2. If a future version revives per-agent or per-tenant context
//! profiles, the `Domain` trait below is the contract to start from.

pub mod traits;

pub use traits::{Domain, DomainConfig, DomainContext, DomainProcessor};
