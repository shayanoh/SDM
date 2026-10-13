//! Port of `FakeLinkResolver.swift`: a [`LinkResolver`] whose `resolve`
//! answer is a closure. (The `vf` / `af` builders live in `support/mod.rs`.)

use std::collections::HashSet;

use async_trait::async_trait;
use sdm_core::{
    LinkResolver, MediaFormat, RefreshedFormat, ResolveError, ResolvedMedia, ResolvedTarget,
    SourceUrl,
};
use tokio_util::sync::CancellationToken;

use super::su;

type ResolveFn = Box<dyn Fn(&SourceUrl) -> Result<ResolvedTarget, ResolveError> + Send + Sync>;
type RefreshFn = Box<dyn Fn(&str) -> Result<RefreshedFormat, ResolveError> + Send + Sync>;

pub struct FakeLinkResolver {
    pub handled_hosts: HashSet<String>,
    resolve_result: ResolveFn,
    pub refresh_result: RefreshFn,
}

impl FakeLinkResolver {
    pub fn new(
        resolve: impl Fn(&SourceUrl) -> Result<ResolvedTarget, ResolveError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handled_hosts: [
                "youtube.com",
                "www.youtube.com",
                "youtu.be",
                "m.youtube.com",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            resolve_result: Box::new(resolve),
            refresh_result: Box::new(|id| {
                Ok(RefreshedFormat::new(
                    su(&format!("https://gv/{id}")),
                    None,
                    id,
                ))
            }),
        }
    }

    pub fn with_handled_hosts(mut self, hosts: &[&str]) -> Self {
        self.handled_hosts = hosts.iter().map(|h| h.to_string()).collect();
        self
    }
}

#[async_trait]
impl LinkResolver for FakeLinkResolver {
    fn can_handle(&self, url: &SourceUrl) -> bool {
        url.as_url()
            .and_then(|parsed| parsed.host_str())
            .is_some_and(|host| self.handled_hosts.contains(host))
    }

    async fn resolve(&self, url: &SourceUrl) -> Result<ResolvedTarget, ResolveError> {
        (self.resolve_result)(url)
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

pub fn single_media(video_id: &str, title: &str, formats: Vec<MediaFormat>) -> ResolvedTarget {
    ResolvedTarget::Single(ResolvedMedia::new(
        "youtube",
        video_id,
        title,
        Some(100.0),
        formats,
    ))
}
