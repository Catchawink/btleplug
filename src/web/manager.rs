use async_trait::async_trait;

use crate::{api, Result};

use super::adapter::Adapter;

/// Implementation of [api::Manager](crate::api::Manager).
#[derive(Clone, Debug)]
pub struct Manager {}

impl Manager {
    pub async fn new() -> Result<Self> {
        Ok(Self {})
    }
}

#[async_trait]
impl api::Manager for Manager {
    type Adapter = Adapter;

    async fn adapters(&self) -> Result<Vec<Adapter>> {
        Ok(vec![Adapter::new().await?])
    }
}
