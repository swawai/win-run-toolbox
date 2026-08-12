pub mod archive;
pub mod declaration;
pub mod environment;
pub mod provider;

pub(crate) mod storage;

pub const PRODUCER_CONTRACT: &str = "swawkit.proj.dev-setup/v2";
pub const PUBLICATION_TOKEN_VARIABLE: &str =
    "SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_PUBLICATION_TOKEN";
