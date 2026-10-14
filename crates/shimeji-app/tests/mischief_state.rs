//! Mischief phase: off, awaiting the grant, on, permission missing.

use shimeji_app::mischief_state::{Input, Phase};

#[test]
fn off_enable_grant_reaches_on_and_revoke_warns() {
    let off = Phase::Off;
    assert!(!off.polls());
    assert!(!off.warns_open_settings());
    assert!(!off.shows_explanation());

    let enabled = off.step(Input::UserEnabled);
    assert_eq!(enabled.phase, Phase::AwaitingPermission);
    assert!(enabled.phase.polls());
    assert!(enabled.phase.shows_explanation());
    assert!(!enabled.phase.warns_open_settings());
    assert!(!enabled.end_carry);

    let granted = enabled.phase.step(Input::Trust(true));
    assert_eq!(granted.phase, Phase::On);
    assert!(granted.phase.polls());
    assert!(!granted.phase.shows_explanation());
    assert!(!granted.phase.warns_open_settings());
    assert!(!granted.end_carry);

    let turned_off = granted.phase.step(Input::UserDisabled);
    assert_eq!(turned_off.phase, Phase::Off);
    assert!(turned_off.end_carry);
    assert!(!turned_off.phase.polls());

    let revoked = granted.phase.step(Input::Trust(false));
    assert_eq!(revoked.phase, Phase::PermissionMissing);
    assert!(revoked.end_carry);
    assert!(revoked.phase.warns_open_settings());
    assert!(!revoked.phase.polls());
    assert!(!revoked.phase.shows_explanation());

    let still = revoked.phase.step(Input::Trust(false));
    assert_eq!(still.phase, Phase::PermissionMissing);
    assert!(!still.end_carry);
    assert!(still.phase.warns_open_settings());

    let back = revoked.phase.step(Input::Trust(true));
    assert_eq!(back.phase, Phase::On);
    assert!(!back.end_carry);

    let dismissed = revoked.phase.step(Input::UserDisabled);
    assert_eq!(dismissed.phase, Phase::Off);
    assert!(!dismissed.phase.warns_open_settings());
    assert!(!dismissed.end_carry);
}

#[test]
fn deny_stays_awaiting_until_the_explanation_is_dismissed_and_never_on_without_a_grant() {
    let awaiting = Phase::Off.step(Input::UserEnabled).phase;
    let denied = awaiting.step(Input::Trust(false));
    assert_eq!(denied.phase, Phase::AwaitingPermission);
    assert!(denied.phase.shows_explanation());
    assert!(denied.phase.polls());
    assert!(!denied.end_carry);
    assert!(!denied.phase.warns_open_settings());

    let again = denied.phase.step(Input::Trust(false));
    assert_eq!(again.phase, Phase::AwaitingPermission);

    let off = again.phase.step(Input::ExplanationDismissed);
    assert_eq!(off.phase, Phase::Off);
    assert!(!off.phase.polls());
    assert!(!off.phase.shows_explanation());
    assert!(!off.end_carry);

    assert_eq!(Phase::Off.step(Input::Trust(true)).phase, Phase::Off);
    assert_eq!(
        Phase::Off.step(Input::UserEnabled).phase,
        Phase::AwaitingPermission
    );
    assert_ne!(Phase::Off.step(Input::UserEnabled).phase, Phase::On);
    assert_eq!(
        Phase::AwaitingPermission.step(Input::UserEnabled).phase,
        Phase::AwaitingPermission
    );
    assert_eq!(
        Phase::AwaitingPermission.step(Input::UserDisabled).phase,
        Phase::Off
    );
    assert_eq!(Phase::On.step(Input::ExplanationDismissed).phase, Phase::On);
    assert_eq!(
        Phase::PermissionMissing
            .step(Input::ExplanationDismissed)
            .phase,
        Phase::PermissionMissing
    );
    assert_eq!(
        Phase::Off.step(Input::ExplanationDismissed).phase,
        Phase::Off
    );
}
