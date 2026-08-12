use super::*;

#[test]
fn cab_references_are_matched_as_ascii_without_case_sensitivity() {
    assert!(contains_ascii_case_insensitive(
        b"prefix SDK_HEADERS.CAB suffix",
        b"sdk_headers.cab"
    ));
    assert!(!contains_ascii_case_insensitive(
        b"prefix sdk_headers.ca suffix",
        b"sdk_headers.cab"
    ));
}

#[test]
fn duplicate_cab_leaves_are_rejected_before_any_install_source_is_staged() {
    let payloads = [
        MsvcPayload::fixture("payload.cab", b"one"),
        MsvcPayload::fixture("PAYLOAD.CAB", b"two"),
    ];

    let error = unique_cab_candidates(&payloads).unwrap_err();

    assert!(error.to_string().contains("duplicate CAB payload leaf"));
}
