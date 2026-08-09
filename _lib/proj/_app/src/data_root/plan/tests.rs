use super::*;
use crate::data_root::record::{ENTRY_RECORD_SCHEMA, EntryRecord, EntryRecordState};

const VOLUME_ID: &str = r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}";

fn identity(value: u128) -> EntryIdentity {
    EntryIdentity::from_parts(VOLUME_ID, format!("{value:032x}")).expect("identity")
}

fn valid_record(name: &str, identity: &EntryIdentity) -> EntryRecordState {
    EntryRecordState::Valid {
        path: PathBuf::from(format!(r"D:\fixture\{name}\_entry.json")),
        record: EntryRecord {
            schema: ENTRY_RECORD_SCHEMA.to_owned(),
            entry_name: name.to_owned(),
            entry_file: Some(format!("{name}.exe")),
            volume_id: identity.volume_id().to_owned(),
            file_id: identity.file_id().to_owned(),
        },
    }
}

fn inventory(
    directory: &str,
    roots: impl IntoIterator<Item = (&'static str, EntryRecordState)>,
) -> DataRootInventory {
    let directory = PathBuf::from(directory);
    DataRootInventory::from_snapshots(
        directory.clone(),
        roots
            .into_iter()
            .map(|(name, record)| (directory.join(name), record))
            .collect(),
    )
}

fn request<'a>(
    identity: &'a EntryIdentity,
    current: &'a DataRootInventory,
) -> DataRootPlanningRequest<'a> {
    DataRootPlanningRequest {
        entry_file: Path::new(r"D:\kit\Favorites\project-one.exe"),
        identity,
        current,
    }
}

#[test]
fn creates_only_when_no_binding_exists() {
    let id = identity(1);
    let current = inventory(r"D:\kit\data", []);
    let plan = plan_data_root(request(&id, &current)).expect("plan");
    assert!(matches!(plan, DataRootPlan::Create { .. }));
}

#[test]
fn derives_the_entry_name_from_the_absolute_entry_file() {
    let id = identity(11);
    let current = inventory(r"D:\kit\data", []);
    let plan = plan_data_root(request(&id, &current)).expect("plan");
    assert_eq!(plan.target().entry_name, "project-one");
    assert!(plan.target().data_root.ends_with("proj.project-one"));

    let relative = DataRootPlanningRequest {
        entry_file: Path::new("project-one.exe"),
        identity: &id,
        current: &current,
    };
    assert!(matches!(
        plan_data_root(relative),
        Err(DataRootPlanError::EntryFileNotAbsolute(_))
    ));
}

#[test]
fn uses_a_matching_candidate_directly() {
    let id = identity(2);
    let current = inventory(
        r"D:\kit\data",
        [("PROJ.PROJECT-ONE", valid_record("PROJECT-ONE", &id))],
    );
    assert!(matches!(
        plan_data_root(request(&id, &current)),
        Ok(DataRootPlan::Direct { .. })
    ));
}

#[test]
fn requires_claim_for_an_unbound_or_copied_same_name_candidate() {
    let id = identity(3);
    let copied_from = identity(4);
    let missing = EntryRecordState::Missing {
        path: PathBuf::from(r"D:\kit\data\proj.project-one\_entry.json"),
    };
    let current = inventory(r"D:\kit\data", [("proj.project-one", missing)]);
    let plan = plan_data_root(request(&id, &current)).expect("missing plan");
    assert!(matches!(
        plan,
        DataRootPlan::ClaimCurrent { ref reason, .. } if reason == "identity record is missing"
    ));

    let current = inventory(
        r"D:\kit\data",
        [(
            "proj.project-one",
            valid_record("project-one", &copied_from),
        )],
    );
    assert!(matches!(
        plan_data_root(request(&id, &current)),
        Ok(DataRootPlan::ClaimCurrent { .. })
    ));
}

#[test]
fn follows_a_renamed_entry_by_file_identity() {
    let id = identity(5);
    let current = inventory(
        r"D:\kit\data",
        [("proj.old-name", valid_record("old-name", &id))],
    );
    let plan = plan_data_root(request(&id, &current)).expect("rename plan");
    assert!(matches!(
        plan,
        DataRootPlan::ClaimRename { source_data_root, .. }
            if source_data_root.ends_with("proj.old-name")
    ));
}

#[test]
fn rejects_a_taken_candidate_when_the_identity_exists_elsewhere() {
    let id = identity(6);
    let other = identity(7);
    let current = inventory(
        r"D:\kit\data",
        [
            ("proj.project-one", valid_record("project-one", &other)),
            ("proj.old-name", valid_record("old-name", &id)),
        ],
    );
    assert!(matches!(
        plan_data_root(request(&id, &current)),
        Err(DataRootPlanError::CandidateCollision { .. })
    ));
}

#[test]
fn rejects_ambiguous_current_identity() {
    let id = identity(9);
    let current = inventory(
        r"D:\kit\data",
        [
            ("proj.first", valid_record("first", &id)),
            ("proj.second", valid_record("second", &id)),
        ],
    );
    assert!(matches!(
        plan_data_root(request(&id, &current)),
        Err(DataRootPlanError::MultipleCurrentBindings(_))
    ));
}
