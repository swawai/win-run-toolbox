use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use swawkit_proj::runtime_release::RuntimeReleaseStore;

use super::CommandContext;

mod lock;
mod process;

use lock::PublicationLock;
use process::InUseReleases;

static NEXT_TOMBSTONE: AtomicU64 = AtomicU64::new(0);

pub(super) fn run(context: &CommandContext, arguments: &[OsString]) -> Result<(), String> {
    let apply = parse_arguments(arguments)?;
    let _lock = PublicationLock::acquire(&context.swawkit_home)?;
    let store = RuntimeReleaseStore::open(&context.swawkit_home)
        .map_err(|error| format!("cannot open Runtime Release storage: {error}"))?;
    let selected = store
        .selected_release_id()
        .map_err(|error| format!("cannot read selected Runtime Release: {error}"))?;
    let in_use = process::in_use_releases(store.releases_root())?;
    let plan = build_plan(&store, &selected, &in_use)?;
    execute_plan(&store, &plan, apply)
}

fn parse_arguments(arguments: &[OsString]) -> Result<bool, String> {
    match arguments {
        [] => Ok(false),
        [argument] if argument == "--apply" => Ok(true),
        _ => Err("usage: .runtime.cleanup [--apply]".to_owned()),
    }
}

#[derive(Debug)]
struct PlanItem {
    name: String,
    root: PathBuf,
    state: PlanState,
}

#[derive(Debug, PartialEq, Eq)]
enum PlanState {
    Selected,
    InUse(Vec<u32>),
    Removable,
    Retained(String),
}

fn build_plan(
    store: &RuntimeReleaseStore,
    selected: &str,
    in_use: &InUseReleases,
) -> Result<Vec<PlanItem>, String> {
    store.validate(selected).map_err(|error| {
        format!("selected Runtime Release '{selected}' is not valid; cleanup stopped: {error}")
    })?;
    let mut entries = fs::read_dir(store.releases_root())
        .map_err(|error| format!("cannot enumerate Runtime Releases: {error}"))?
        .map(|entry| entry.map_err(|error| format!("cannot enumerate Runtime Releases: {error}")))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    let mut plan = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let root = entry.path();
        let validation = store.validate(&name);
        let state = if name == selected {
            PlanState::Selected
        } else if let Err(error) = validation {
            PlanState::Retained(error.to_string())
        } else if let Some(pids) = in_use.get(&name) {
            PlanState::InUse(pids.clone())
        } else {
            PlanState::Removable
        };
        plan.push(PlanItem { name, root, state });
    }
    Ok(plan)
}

fn execute_plan(store: &RuntimeReleaseStore, plan: &[PlanItem], apply: bool) -> Result<(), String> {
    let mut selected = 0_usize;
    let mut in_use = 0_usize;
    let mut removable = 0_usize;
    let mut retained = 0_usize;
    let mut removed = 0_usize;

    println!(
        "Runtime Release cleanup {}",
        if apply { "apply" } else { "preview" }
    );
    for item in plan {
        match &item.state {
            PlanState::Selected => {
                selected += 1;
                println!("[SELECTED] {}", item.name);
            }
            PlanState::InUse(pids) => {
                in_use += 1;
                println!("[IN USE] {} PID {}", item.name, join_pids(pids));
            }
            PlanState::Retained(reason) => {
                retained += 1;
                println!("[RETAINED] {}: {reason}", item.name);
            }
            PlanState::Removable if !apply => {
                removable += 1;
                println!("[REMOVABLE] {}", item.name);
            }
            PlanState::Removable => {
                let latest = process::in_use_releases(store.releases_root())?;
                if let Some(pids) = latest.get(&item.name) {
                    in_use += 1;
                    println!("[IN USE] {} PID {}", item.name, join_pids(pids));
                    continue;
                }
                remove_release(store, item)?;
                removed += 1;
                println!("[REMOVED] {}", item.name);
            }
        }
    }
    println!(
        "Summary: selected={selected}, in-use={in_use}, removable={removable}, \
         removed={removed}, retained={retained}"
    );
    if !apply && removable > 0 {
        println!("Run again with --apply to delete the removable Releases.");
    }
    Ok(())
}

fn remove_release(store: &RuntimeReleaseStore, item: &PlanItem) -> Result<(), String> {
    let validated = store.validate(&item.name).map_err(|error| {
        format!(
            "Runtime Release changed after preview and was not removed '{}': {error}",
            item.root.display()
        )
    })?;
    if validated.root != item.root {
        return Err("Runtime Release path changed after preview".to_owned());
    }
    let tombstone = tombstone_path(store.releases_root(), &item.name);
    if tombstone.exists() {
        return Err(format!(
            "Runtime cleanup tombstone already exists: {}",
            tombstone.display()
        ));
    }
    fs::rename(&item.root, &tombstone).map_err(|error| {
        format!(
            "cannot isolate Runtime Release '{}': {error}",
            item.root.display()
        )
    })?;
    fs::remove_dir_all(&tombstone).map_err(|error| {
        format!(
            "Runtime Release was isolated but could not be fully deleted; retain and inspect '{}': \
             {error}",
            tombstone.display()
        )
    })
}

fn tombstone_path(releases_root: &Path, release_id: &str) -> PathBuf {
    let sequence = NEXT_TOMBSTONE.fetch_add(1, Ordering::Relaxed);
    releases_root.join(format!(
        ".cleanup-{release_id}-{}-{sequence}.tmp",
        std::process::id()
    ))
}

fn join_pids(pids: &[u32]) -> String {
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests;
