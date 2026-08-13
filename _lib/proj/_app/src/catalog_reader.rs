use std::io;

use crate::{catalog::CatalogSnapshot, context::EntryContext, profile::EntryProfileStore};

#[derive(Debug, Clone)]
pub struct CatalogReader {
    context: EntryContext,
    profile_store: EntryProfileStore,
}

impl CatalogReader {
    pub fn new(context: EntryContext, profile_store: EntryProfileStore) -> Self {
        Self {
            context,
            profile_store,
        }
    }

    pub async fn read(&self) -> io::Result<CatalogSnapshot> {
        let context = self.context.clone();
        let profile_store = self.profile_store.clone();
        tokio::task::spawn_blocking(move || {
            let state = profile_store.read();
            CatalogSnapshot::discover(&context, state.ready())
        })
        .await
        .map_err(|error| io::Error::other(format!("catalog worker failed: {error}")))?
    }
}
