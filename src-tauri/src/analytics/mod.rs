//! Analytics hub projection + per-user executive dashboard layouts (device-local).

pub mod as_of;
pub mod layout;
pub mod overview_kpis;
pub mod projection;
pub mod smrp;

#[cfg(test)]
mod smrp_tests;
#[cfg(test)]
mod tests;
