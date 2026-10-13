//! Port of `FakeResolver` from Swift's `URLRefreshTests.swift`.

use async_trait::async_trait;
use sdm_core::{LinkResolver, RefreshedFormat, ResolveError, ResolvedTarget, SourceUrl};
use tokio_util::sync::CancellationToken;

type RefreshResult = Box<dyn Fn(&str) -> Result<RefreshedFormat, ResolveError> + Send + Sync>;

/// Hands back a scripted `RefreshedFormat` (or an error) — the engine's only
/// use of a `LinkResolver` is `refresh`.
pub struct FakeResolver {
    refresh_result: RefreshResult,
}

impl FakeResolver {
    pub fn new(
        refresh_result: impl Fn(&str) -> Result<RefreshedFormat, ResolveError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            refresh_result: Box::new(refresh_result),
        }
    }
}

#[async_trait]
impl LinkResolver for FakeResolver {
    fn can_handle(&self, _url: &SourceUrl) -> bool {
        false
    }

    async fn resolve(&self, _url: &SourceUrl) -> Result<ResolvedTarget, ResolveError> {
        Err(ResolveError::Unsupported)
    }

    async fn refresh(
        &self,
        _source_url: &SourceUrl,
        format_id: &str,
        _cancel: &CancellationToken,
    ) -> Result<RefreshedFormat, ResolveError> {
        (self.refresh_result)(format_id)
    }
}
