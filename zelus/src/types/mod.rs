// SPDX-License-Identifier: AGPL-3.0-only
mod jsonvec;
mod stream;

pub use jsonvec::JsonVec;
pub use stream::{DataStream, DatastreamReqwestExt};
use utoipa::openapi::path::OperationBuilder;

pub trait DocumentedType {
    #[must_use]
    fn openapi(operations: OperationBuilder) -> OperationBuilder {
        operations
    }
}
