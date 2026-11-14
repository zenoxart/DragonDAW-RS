//! Pure rollout rules.

use redhive_contracts::modules::{RolloutInfo, RolloutStage};
use redhive_monads::{Error, Res};
use uuid::Uuid;

/// First stage: held while workflows would break, complete for first versions / immediate, else pilot.
pub fn start_stage(broken_workflows: i32, immediate: bool) -> (RolloutStage, Option<String>) {
    if broken_workflows > 0 {
        (
            RolloutStage::Held,
            Some(format!("{broken_workflows} workflow(s) would break with this version. Review them in the impact overview, then resume the rollout.")),
        )
    } else if immediate {
        (RolloutStage::Complete, None)
    } else {
        (RolloutStage::Pilot, None)
    }
}

pub fn advance_stage(r: &RolloutInfo, force: bool) -> Res<RolloutStage> {
    match r.stage {
        RolloutStage::Held if r.broken_workflows > 0 && !force => Err(Error::conflict("updates.rollout.broken", "updates:rollout.broken").with_metadata("broken", r.broken_workflows)),
        RolloutStage::Held => Ok(RolloutStage::Pilot),
        RolloutStage::Pilot => Ok(RolloutStage::Broad),
        RolloutStage::Broad => Ok(RolloutStage::Complete),
        RolloutStage::Complete | RolloutStage::Cancelled => Err(Error::conflict("updates.rollout.finished", "updates:rollout.finished")),
    }
}

pub fn hold_stage(r: &RolloutInfo) -> Res<RolloutStage> {
    match r.stage {
        RolloutStage::Pilot | RolloutStage::Broad => Ok(RolloutStage::Held),
        _ => Err(Error::conflict("updates.rollout.notActive", "updates:rollout.notActive")),
    }
}

pub fn cancel_stage(r: &RolloutInfo) -> Res<RolloutStage> {
    match r.stage {
        RolloutStage::Complete | RolloutStage::Cancelled => Err(Error::conflict("updates.rollout.finished", "updates:rollout.finished")),
        _ => Ok(RolloutStage::Cancelled),
    }
}

/// The version a user gets while a rollout is running.
pub fn version_for(r: &RolloutInfo, user_id: Uuid, user_roles: &[String]) -> String {
    let previous = r.from_version.clone().unwrap_or_else(|| r.to_version.clone());
    match r.stage {
        RolloutStage::Complete => r.to_version.clone(),
        RolloutStage::Pilot if r.pilot_users.contains(&user_id) => r.to_version.clone(),
        RolloutStage::Broad if r.pilot_users.contains(&user_id) || r.broad_roles.is_empty() || r.broad_roles.iter().any(|b| user_roles.contains(b)) => r.to_version.clone(),
        // Held keeps whoever already had it on the new version only for the pilot group (no new swaps).
        RolloutStage::Held if r.pilot_users.contains(&user_id) && r.broken_workflows == 0 => r.to_version.clone(),
        _ => previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn rollout(stage: RolloutStage) -> RolloutInfo {
        RolloutInfo {
            id: Uuid::nil(),
            module_id: "m".into(),
            from_version: Some("1.0.0".into()),
            to_version: "1.1.0".into(),
            stage,
            hold_reason: None,
            impact_report_id: None,
            broken_workflows: 0,
            pilot_users: vec![Uuid::from_u128(1)],
            broad_roles: vec!["Approver".into()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn stages() {
        assert_eq!(start_stage(2, false).0, RolloutStage::Held);
        assert_eq!(start_stage(0, true).0, RolloutStage::Complete);
        let mut held = rollout(RolloutStage::Held);
        held.broken_workflows = 1;
        assert!(advance_stage(&held, false).is_err());
        assert_eq!(advance_stage(&held, true).unwrap(), RolloutStage::Pilot);
        assert_eq!(advance_stage(&rollout(RolloutStage::Pilot), false).unwrap(), RolloutStage::Broad);
    }

    #[test]
    fn versions_per_user() {
        let p = rollout(RolloutStage::Pilot);
        assert_eq!(version_for(&p, Uuid::from_u128(1), &[]), "1.1.0");
        assert_eq!(version_for(&p, Uuid::from_u128(2), &[]), "1.0.0");
        let b = rollout(RolloutStage::Broad);
        assert_eq!(version_for(&b, Uuid::from_u128(2), &["Approver".into()]), "1.1.0");
        assert_eq!(version_for(&b, Uuid::from_u128(2), &["Clerk".into()]), "1.0.0");
        assert_eq!(version_for(&rollout(RolloutStage::Cancelled), Uuid::from_u128(1), &[]), "1.0.0");
    }
}
