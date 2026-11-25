//! C-079: recompute planning after a method change (filled in below).

/// What the automatic recompute planning is told (`VYREON_CORE_RECOMPUTE_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecomputeWatch {
    /// The price of a worker node-hour, USD (the pools' list price), for the plan's cost against the ceiling.
    pub node_hour_usd: f64,
    /// The days a full recompute's day block covers.
    pub block: i64,
}
