use super::{RustDefinition, RustInstallation};
use crate::development::setup::environment::EnvironmentPlan;

impl RustInstallation {
    pub fn add_environment(
        &self,
        definition: &RustDefinition,
        plan: &mut EnvironmentPlan,
    ) -> Result<(), String> {
        let cargo_home = self.root.join("cargo");
        let rustup_home = self.root.join("rustup");
        let toolchain_bin = rustup_home
            .join("toolchains")
            .join(definition.toolchain_name())
            .join("bin");
        let rustc = toolchain_bin
            .join("rustc.exe")
            .to_string_lossy()
            .into_owned();
        let rustdoc = toolchain_bin
            .join("rustdoc.exe")
            .to_string_lossy()
            .into_owned();
        for (name, value) in [
            ("RUSTUP_HOME", rustup_home.to_string_lossy().into_owned()),
            ("CARGO_HOME", cargo_home.to_string_lossy().into_owned()),
            ("RUSTUP_TOOLCHAIN", definition.toolchain_name().to_owned()),
            ("RUSTC", rustc.clone()),
            ("RUSTDOC", rustdoc.clone()),
            ("CARGO_BUILD_RUSTC", rustc),
            ("CARGO_BUILD_RUSTDOC", rustdoc),
        ] {
            plan.set(name, Some(value))?;
        }
        for name in [
            "RUSTUP_TOOLCHAIN_SOURCE",
            "RUSTUP_DIST_SERVER",
            "RUSTUP_DIST_ROOT",
            "RUSTUP_UPDATE_ROOT",
            "RUSTUP_VERSION",
        ] {
            plan.set(name, None::<String>)?;
        }
        plan.prepend_path(cargo_home.join("bin"))
    }
}
