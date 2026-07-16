use std::future::Future;
use std::pin::Pin;

use crate::environment::types::ResourceLocator;
use crate::error::AppError;

pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait AtomicDocumentIo: Send + Sync {
    fn read_optional<'a>(
        &'a self,
        target: &'a ResourceLocator,
    ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>>;

    #[cfg(test)]
    fn backup_exists<'a>(
        &'a self,
        target: &'a ResourceLocator,
    ) -> IoFuture<'a, Result<bool, AppError>>;

    fn write_atomic<'a>(
        &'a self,
        target: &'a ResourceLocator,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<(), AppError>>;
}
