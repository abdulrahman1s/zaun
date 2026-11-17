//! Run a query on the job pool and send its answer back as `FlowDone::Query` (ADR-0012).
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn answer_on_pool(
        &self,
        label: &'static str,
        id: QueryId,
        f: impl FnOnce(&crate::jobs::CancelToken) -> QueryResult + Send + 'static,
    ) {
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel(label), move |c| {
                let result = f(c);
                post(Internal::Flow(Box::new(FlowDone::Query { id, result })));
            });
    }
}
