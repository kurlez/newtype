//! Path router. UI code asks this module which path a dictation should take.
//!
//! Local models are not linked in this build. The decision still names the
//! local path so a later milestone can turn `local_asr_ready` on without
//! scattering that choice through the session.

use serde::{Deserialize, Serialize};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "router",
        status: "ready",
        note: "Cloud is the live path. Local fallback is decided here and is not linked yet.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RoutePreference {
    #[default]
    CloudDefault,
    LocalOnly,
    StrictOffline,
}

impl RoutePreference {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cloudDefault" | "cloud" => Some(Self::CloudDefault),
            "localOnly" | "local" => Some(Self::LocalOnly),
            "strictOffline" | "offline" => Some(Self::StrictOffline),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CloudDefault => "cloudDefault",
            Self::LocalOnly => "localOnly",
            Self::StrictOffline => "strictOffline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathLabel {
    Cloud,
    LocalAutoFallback,
    LocalManual,
    StrictOffline,
}

impl PathLabel {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cloud => "cloud",
            Self::LocalAutoFallback => "localAuto",
            Self::LocalManual => "localManual",
            Self::StrictOffline => "strictOffline",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Cloud => "Cloud · 云端",
            Self::LocalAutoFallback => "Local · auto fallback · 本地 · 自动回落",
            Self::LocalManual => "Local · manual · 本地 · 手动",
            Self::StrictOffline => "Strict offline · 严格离线",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RouteReason {
    OnlineCloud,
    OfflineFallback,
    ManualLocal,
    CloudFailedFallback,
    StrictOffline,
    LocalNotReady,
    CloudNotConfigured,
    AsrNotFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    /// No probe has run. This build tries the cloud path.
    Unknown,
    Reachable,
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Cloud,
    Local,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    None,
    CloudNotConfigured,
    LocalNotReady,
    AsrNotFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteInput {
    pub preference: RoutePreference,
    pub reachability: Reachability,
    pub cloud_configured: bool,
    pub asr_implemented: bool,
    pub local_asr_ready: bool,
    pub local_llm_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDecision {
    pub asr: Slot,
    pub llm: Slot,
    pub label: PathLabel,
    pub reason: RouteReason,
    pub block: Block,
}

pub fn decide(input: RouteInput) -> RouteDecision {
    match input.preference {
        RoutePreference::StrictOffline => local_or_blocked(
            input,
            PathLabel::StrictOffline,
            RouteReason::StrictOffline,
        ),
        RoutePreference::LocalOnly => {
            local_or_blocked(input, PathLabel::LocalManual, RouteReason::ManualLocal)
        }
        RoutePreference::CloudDefault => decide_cloud(input),
    }
}

fn local_or_blocked(input: RouteInput, label: PathLabel, reason: RouteReason) -> RouteDecision {
    if input.local_asr_ready {
        RouteDecision {
            asr: Slot::Local,
            llm: if input.local_llm_ready {
                Slot::Local
            } else {
                Slot::None
            },
            label,
            reason,
            block: Block::None,
        }
    } else {
        RouteDecision {
            asr: Slot::None,
            llm: Slot::None,
            label,
            reason: RouteReason::LocalNotReady,
            block: Block::LocalNotReady,
        }
    }
}

fn decide_cloud(input: RouteInput) -> RouteDecision {
    let online = matches!(
        input.reachability,
        Reachability::Reachable | Reachability::Unknown
    );
    if !online {
        return if input.local_asr_ready {
            RouteDecision {
                asr: Slot::Local,
                llm: if input.local_llm_ready {
                    Slot::Local
                } else {
                    Slot::None
                },
                label: PathLabel::LocalAutoFallback,
                reason: RouteReason::OfflineFallback,
                block: Block::None,
            }
        } else {
            RouteDecision {
                asr: Slot::None,
                llm: Slot::None,
                label: PathLabel::Cloud,
                reason: RouteReason::LocalNotReady,
                block: Block::LocalNotReady,
            }
        };
    }
    if !input.cloud_configured {
        return if input.local_asr_ready {
            RouteDecision {
                asr: Slot::Local,
                llm: if input.local_llm_ready {
                    Slot::Local
                } else {
                    Slot::None
                },
                label: PathLabel::LocalAutoFallback,
                reason: RouteReason::CloudNotConfigured,
                block: Block::None,
            }
        } else {
            RouteDecision {
                asr: Slot::None,
                llm: Slot::None,
                label: PathLabel::Cloud,
                reason: RouteReason::CloudNotConfigured,
                block: Block::CloudNotConfigured,
            }
        };
    }
    if !input.asr_implemented {
        return RouteDecision {
            asr: Slot::None,
            llm: Slot::None,
            label: PathLabel::Cloud,
            reason: RouteReason::AsrNotFinished,
            block: Block::AsrNotFinished,
        };
    }
    RouteDecision {
        asr: Slot::Cloud,
        llm: Slot::Cloud,
        label: PathLabel::Cloud,
        reason: RouteReason::OnlineCloud,
        block: Block::None,
    }
}

pub fn block_message(block: Block) -> Option<&'static str> {
    match block {
        Block::None => None,
        Block::CloudNotConfigured => {
            Some("Save an API key for the speech provider before dictating. Audio is not sent until you do.")
        }
        Block::LocalNotReady => {
            Some("Local models are not in this build, so this path cannot dictate yet. Cloud is the working path.")
        }
        Block::AsrNotFinished => {
            Some("This speech provider is not finished. Pick OpenAI, Groq, DashScope, or a custom endpoint for speech.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(preference: RoutePreference) -> RouteInput {
        RouteInput {
            preference,
            reachability: Reachability::Unknown,
            cloud_configured: true,
            asr_implemented: true,
            local_asr_ready: false,
            local_llm_ready: false,
        }
    }

    #[test]
    fn cloud_default_uses_cloud_until_a_probe_says_otherwise() {
        let decision = decide(input(RoutePreference::CloudDefault));
        assert_eq!(decision.asr, Slot::Cloud);
        assert_eq!(decision.llm, Slot::Cloud);
        assert_eq!(decision.label, PathLabel::Cloud);
        assert_eq!(decision.block, Block::None);
    }

    #[test]
    fn unreachable_cloud_falls_back_only_when_local_is_ready() {
        let mut offline = input(RoutePreference::CloudDefault);
        offline.reachability = Reachability::Unreachable;
        let blocked = decide(offline);
        assert_eq!(blocked.block, Block::LocalNotReady);
        assert_eq!(blocked.asr, Slot::None);

        offline.local_asr_ready = true;
        let fallback = decide(offline);
        assert_eq!(fallback.label, PathLabel::LocalAutoFallback);
        assert_eq!(fallback.asr, Slot::Local);
        assert_eq!(fallback.llm, Slot::None);
        assert_eq!(fallback.reason, RouteReason::OfflineFallback);
    }

    #[test]
    fn strict_offline_never_selects_cloud() {
        let mut strict = input(RoutePreference::StrictOffline);
        strict.reachability = Reachability::Reachable;
        strict.local_asr_ready = true;
        strict.local_llm_ready = true;
        let decision = decide(strict);
        assert_eq!(decision.label, PathLabel::StrictOffline);
        assert_eq!(decision.asr, Slot::Local);
        assert_ne!(decision.asr, Slot::Cloud);
        assert_eq!(decision.llm, Slot::Local);

        strict.local_asr_ready = false;
        let blocked = decide(strict);
        assert_eq!(blocked.block, Block::LocalNotReady);
        assert_ne!(blocked.asr, Slot::Cloud);
    }

    #[test]
    fn manual_local_stays_local_when_the_cloud_is_reachable() {
        let mut local = input(RoutePreference::LocalOnly);
        local.reachability = Reachability::Reachable;
        local.local_asr_ready = true;
        let decision = decide(local);
        assert_eq!(decision.label, PathLabel::LocalManual);
        assert_eq!(decision.reason, RouteReason::ManualLocal);
        assert_eq!(decision.asr, Slot::Local);
    }

    #[test]
    fn missing_key_or_unfinished_asr_does_not_pretend_to_send_audio() {
        let mut bare = input(RoutePreference::CloudDefault);
        bare.cloud_configured = false;
        assert_eq!(decide(bare).block, Block::CloudNotConfigured);

        bare.cloud_configured = true;
        bare.asr_implemented = false;
        assert_eq!(decide(bare).block, Block::AsrNotFinished);
        assert_eq!(decide(bare).asr, Slot::None);
    }
}
