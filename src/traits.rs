use crate::Error;
use async_trait::async_trait;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait ParamsReader {
    async fn get<T>(&mut self, root: impl Into<String> + Send) -> Result<T, Error>
    where
        T: DeserializeOwned + Send + Default + 'static;
}
