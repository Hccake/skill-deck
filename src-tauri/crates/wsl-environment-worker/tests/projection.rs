use environment_protocol::{ProjectionRequest, MAX_INSPECTION_ROOTS};

#[test]
fn oversized_projection_reports_the_request_limit() {
    let error = wsl_environment_worker::execute_projection(
        ProjectionRequest {
            destinations: (0..=MAX_INSPECTION_ROOTS)
                .map(|index| format!("/tmp/skill-deck-projection-{index}"))
                .collect(),
            deadline_millis: 10_000,
        },
        || false,
    )
    .unwrap_err();

    assert_eq!(error.phase, "projection");
    assert_eq!(error.code, "requestTooLarge");
}
