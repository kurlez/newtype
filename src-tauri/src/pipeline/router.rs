#![allow(dead_code)]

//! Path router. PRD §4.7 and §7.4. Not implemented.
//!
//! Later milestones choose cloud, local fallback, manual local, or strict offline here.
//! UI code must not duplicate that decision.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "router",
        status: "stub",
        note: "Cloud / local fallback / strict offline routing is not active.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePreference {
    CloudDefault,
    LocalOnly,
    StrictOffline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathLabel {
    Cloud,
    LocalAutoFallback,
    LocalManual,
    StrictOffline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteReason {
    OnlineCloud,
    OfflineFallback,
    ManualLocal,
    CloudFailedFallback,
    StrictOffline,
    LocalNotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDecision {
    pub label: PathLabel,
    pub reason: RouteReason,
}
