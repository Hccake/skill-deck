use std::sync::{Arc, Mutex};

use super::RuntimeAdmissionCoordinator;
use crate::core::mutation::{
    LifecycleLeaseKind, MutationKind, MutationPhase, MutationProgress, MutationSnapshot,
    TerminationAdmission,
};
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::error::AppError;

fn native_global() -> SkillLocationRef {
    SkillLocationRef {
        environment: EnvironmentRef::Native,
        scope: SkillLocation::Global,
    }
}

#[test]
fn lifecycle_and_mutation_are_mutually_exclusive() {
    let admission = RuntimeAdmissionCoordinator::default();
    let lease = admission
        .begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate)
        .expect("begin lifecycle");
    assert_eq!(admission.activity_snapshot().revision, 1);
    assert!(matches!(
        admission.begin_mutation(MutationKind::Install, native_global()),
        Err(AppError::MutationBusy)
    ));
    assert!(matches!(
        admission.request_termination(),
        TerminationAdmission::Blocked(snapshot)
            if snapshot.lifecycle.is_some() && snapshot.mutation.is_none()
    ));
    drop(lease);
    assert_eq!(admission.activity_snapshot().revision, 2);

    let mutation = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");
    assert!(matches!(
        admission.begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance),
        Err(AppError::MutationBusy)
    ));
    drop(mutation);
}

#[test]
fn snapshot_revision_tracks_the_mutation_lifecycle() {
    let admission = RuntimeAdmissionCoordinator::default();
    assert_eq!(
        admission.snapshot(),
        MutationSnapshot {
            revision: 0,
            active: None,
        }
    );

    let permit = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");
    assert_eq!(admission.snapshot().revision, 1);
    permit.transition(
        MutationPhase::Acquiring,
        Some(MutationProgress {
            subject: Some("demo".to_string()),
            current: Some(1),
            total: Some(3),
        }),
        false,
    );
    assert_eq!(admission.snapshot().revision, 2);
    drop(permit);
    assert_eq!(
        admission.snapshot(),
        MutationSnapshot {
            revision: 3,
            active: None,
        }
    );
}

#[test]
fn rejected_mutation_does_not_advance_snapshot_revision() {
    let admission = RuntimeAdmissionCoordinator::default();
    let _permit = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");

    assert!(admission
        .begin_mutation(MutationKind::Remove, native_global())
        .is_err());
    assert_eq!(admission.snapshot().revision, 1);
}

#[test]
fn mutation_changes_publish_complete_snapshots() {
    let admission = RuntimeAdmissionCoordinator::default();
    let revisions = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&revisions);
    admission.set_mutation_listener(move |snapshot| {
        observed.lock().expect("observed revisions lock").push((
            snapshot.revision,
            snapshot.active.as_ref().map(|active| active.phase),
        ));
    });

    let permit = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");
    permit.transition(MutationPhase::Committing, None, false);
    drop(permit);

    assert_eq!(
        *revisions.lock().expect("revisions lock"),
        vec![
            (1, Some(MutationPhase::Preparing)),
            (2, Some(MutationPhase::Committing)),
            (3, None),
        ]
    );
}

#[test]
fn mutation_permit_releases_the_single_active_slot() {
    let admission = RuntimeAdmissionCoordinator::default();
    {
        let _permit = admission
            .begin_mutation(MutationKind::Install, native_global())
            .expect("begin mutation");
        assert!(admission
            .begin_mutation(MutationKind::Remove, native_global())
            .is_err());
        assert_eq!(
            admission.active().expect("active mutation").kind,
            MutationKind::Install
        );
    }
    assert!(admission.active().is_none());
    admission
        .begin_mutation(MutationKind::Update, native_global())
        .expect("begin update after release");
}

#[test]
fn transition_publishes_progress_and_controls_cancellation() {
    let admission = RuntimeAdmissionCoordinator::default();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_observed = Arc::clone(&observed);
    admission.set_mutation_listener(move |snapshot| {
        listener_observed
            .lock()
            .expect("observed snapshots lock")
            .push(snapshot);
    });
    let permit = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");
    assert!(!admission.active().expect("active mutation").cancelable);
    observed.lock().expect("observed snapshots lock").clear();
    let progress = MutationProgress {
        subject: Some("toolkit".to_string()),
        current: Some(1),
        total: Some(2),
    };

    permit.transition(MutationPhase::Acquiring, Some(progress.clone()), true);

    let snapshots = observed.lock().expect("observed snapshots lock");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].revision, 2);
    assert_eq!(
        snapshots[0].active.as_ref().expect("active").progress,
        Some(progress)
    );
    assert!(snapshots[0].active.as_ref().expect("active").cancelable);
    drop(snapshots);
    assert!(admission.request_cancel().expect("request cancel"));
    assert!(permit.cancellation().is_cancelled());

    permit.transition(MutationPhase::Committing, None, true);
    assert!(!admission.active().expect("active").cancelable);
    assert!(!admission.request_cancel().expect("request cancel"));
    permit.transition(MutationPhase::Finishing, None, true);
    assert!(!admission.active().expect("active").cancelable);
}

#[test]
fn termination_blocks_active_work_and_rejects_new_work_after_admission() {
    let admission = RuntimeAdmissionCoordinator::default();
    let permit = admission
        .begin_mutation(MutationKind::Install, native_global())
        .expect("begin mutation");
    assert!(matches!(
        admission.request_termination(),
        TerminationAdmission::Blocked(snapshot)
            if snapshot.mutation.as_ref().map(|active| active.kind) == Some(MutationKind::Install)
    ));
    drop(permit);

    assert_eq!(
        admission.request_termination(),
        TerminationAdmission::Acquired
    );
    assert_eq!(
        admission.request_termination(),
        TerminationAdmission::AlreadyRequested
    );
    assert_eq!(admission.activity_snapshot().revision, 3);
    assert!(matches!(
        admission.begin_mutation(MutationKind::Update, native_global()),
        Err(AppError::ApplicationTerminating)
    ));
}

#[test]
fn idle_action_is_blocked_by_active_mutation() {
    let admission = RuntimeAdmissionCoordinator::default();
    let permit = admission
        .begin_mutation(MutationKind::Remove, native_global())
        .expect("begin mutation");
    let mut performed = false;

    assert!(admission.with_idle(|| performed = true).is_err());
    assert!(!performed);

    drop(permit);
    assert_eq!(admission.with_idle(|| 42), Ok(42));
}
