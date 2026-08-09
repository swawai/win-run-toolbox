use std::ffi::OsString;
use swawkit_proj::{
    catalog::{CatalogSnapshot, is_help_marker},
    context::EntryContext,
    data_root::{
        ClaimApprovalError, DataRootClaim, DataRootClaimDocument, ResolveDataRootRequest,
        claim_data_root, inspect_data_root,
    },
    help::render_help,
};

use super::{CliError, control, write_output};

const ADDRESS: &str = "..entry.claim";

pub(super) fn run(
    context: &EntryContext,
    argv: &[OsString],
    snapshot: &CatalogSnapshot,
    address: &str,
) -> Result<i32, CliError> {
    let command = control::resolve_control(snapshot, address)?;
    if command.handler.as_deref() != Some("entry.claim") {
        return Err(CliError::new(format!(
            "Catalog invariant failed for '{address}': expected the entry.claim handler"
        )));
    }

    let arguments = argv.get(1..).unwrap_or_default();
    if matches!(arguments, [marker] if marker.to_str().is_some_and(is_help_marker)) {
        let output =
            render_help(snapshot, address).map_err(|error| CliError::new(error.to_string()))?;
        write_output(&output)
            .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        return Ok(0);
    }

    let mode = parse_mode(arguments, address)?;
    let request = ResolveDataRootRequest {
        swawkit_home: &context.swawkit_home,
        entry_file: &context.entry_file,
    };
    let inspection = inspect_data_root(request)
        .map_err(|error| CliError::new(format!("DataRoot inspection failed: {error}")))?;

    match mode {
        ClaimMode::Preview => write_human_preview(context, address, inspection.claim.as_ref())?,
        ClaimMode::Json => write_json_preview(inspection.claim.as_ref())?,
        ClaimMode::Apply => {
            let Some(expected) = inspection.claim else {
                write_output(
                    "DataRoot Claim\nStatus: notRequired\nNo ownership claim is required.",
                )
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
                return Ok(0);
            };
            let resolved = claim_data_root(request, &expected)
                .map_err(|error| CliError::new(format!("DataRoot claim failed: {error}")))?;
            let output = format!(
                "DataRoot Claim\nStatus: claimed\nEntry: {}\nDataRoot: {}",
                expected.entry_name,
                resolved.path().display()
            );
            write_output(&output)
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        }
    }
    Ok(0)
}

pub(super) fn rejection(context: &EntryContext, claim: &DataRootClaim) -> ClaimApprovalError {
    ClaimApprovalError::new(format!(
        "DataRoot ownership claim is required.\n{}",
        human_claim(context, ADDRESS, claim, true)
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimMode {
    Preview,
    Json,
    Apply,
}

fn parse_mode(arguments: &[OsString], address: &str) -> Result<ClaimMode, CliError> {
    match arguments {
        [] => Ok(ClaimMode::Preview),
        [value] if value == "--dry-run" => Ok(ClaimMode::Preview),
        [value] if value == "--json" => Ok(ClaimMode::Json),
        [value] if value == "--yes" => Ok(ClaimMode::Apply),
        _ => Err(CliError::new(format!(
            "usage: {address} [--dry-run | --json | --yes]"
        ))),
    }
}

fn write_human_preview(
    context: &EntryContext,
    address: &str,
    claim: Option<&DataRootClaim>,
) -> Result<(), CliError> {
    let output = match claim {
        Some(claim) => human_claim(context, address, claim, true),
        None => "DataRoot Claim\nStatus: notRequired\nNo ownership claim is required.".to_owned(),
    };
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

fn human_claim(
    context: &EntryContext,
    address: &str,
    claim: &DataRootClaim,
    commands: bool,
) -> String {
    let mut output = format!(
        concat!(
            "DataRoot Claim\n",
            "Status: claimRequired\n",
            "Kind: {}\n",
            "Entry: {}\n",
            "Entry File: {}\n",
            "Volume ID: {}\n",
            "File ID: {}\n",
            "Target: {}\n"
        ),
        claim.kind.as_str(),
        claim.entry_name,
        claim.entry_file.display(),
        claim.volume_id,
        claim.file_id,
        claim.data_root.display(),
    );
    if let Some(source) = &claim.source_data_root {
        output.push_str(&format!("Source: {}\n", source.display()));
    }
    output.push_str(&format!("Reason: {}", claim.reason));
    if commands {
        output.push_str(&format!(
            "\nReview: {} {}\nApply: {} {} --yes",
            context.entry_name, address, context.entry_name, address
        ));
    }
    output
}

fn write_json_preview(claim: Option<&DataRootClaim>) -> Result<(), CliError> {
    let document = DataRootClaimDocument::inspect(claim);
    let output = serde_json::to_string_pretty(&document)
        .map_err(|error| CliError::new(format!("cannot serialize DataRoot claim: {error}")))?;
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_modes_are_explicit_and_force_is_not_a_bypass() {
        assert_eq!(parse_mode(&[], ADDRESS).unwrap(), ClaimMode::Preview);
        assert_eq!(
            parse_mode(&[OsString::from("--dry-run")], ADDRESS).unwrap(),
            ClaimMode::Preview
        );
        assert_eq!(
            parse_mode(&[OsString::from("--json")], ADDRESS).unwrap(),
            ClaimMode::Json
        );
        assert_eq!(
            parse_mode(&[OsString::from("--yes")], ADDRESS).unwrap(),
            ClaimMode::Apply
        );
        assert!(parse_mode(&[OsString::from("--force")], ADDRESS).is_err());
        assert!(
            parse_mode(
                &[OsString::from("--dry-run"), OsString::from("--yes")],
                ADDRESS,
            )
            .is_err()
        );
    }
}
