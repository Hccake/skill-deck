use crate::application::wellknown_access::{
    WellKnownAccess, WellKnownCheckFuture, WellKnownFetchFuture,
};
use crate::core::mutation::CancellationSignal;
use crate::runtime::http_transport::HttpTransport;
use crate::runtime::wellknown_protocol::fetch_wellknown_skills_with_client;

pub(crate) struct RuntimeWellKnownAccess {
    http: HttpTransport,
}

impl RuntimeWellKnownAccess {
    pub(crate) fn new(http: HttpTransport) -> Self {
        Self { http }
    }
}

impl WellKnownAccess for RuntimeWellKnownAccess {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        cancellation: &'a CancellationSignal,
    ) -> WellKnownFetchFuture<'a> {
        Box::pin(fetch_wellknown_skills_with_client(
            &self.http,
            url,
            cancellation,
        ))
    }

    fn check<'a>(
        &'a self,
        url: &'a str,
        skill_names: &'a [String],
        cancellation: &'a CancellationSignal,
    ) -> WellKnownCheckFuture<'a> {
        Box::pin(
            crate::runtime::wellknown_protocol::check_wellknown_updates_with_client(
                &self.http,
                url,
                skill_names,
                cancellation,
            ),
        )
    }
}
