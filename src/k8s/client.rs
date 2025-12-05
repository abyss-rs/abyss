use anyhow::Result;
use kube::{Client, Config};

pub struct K8sClient {
    client: Client,
    current_namespace: String,
}

impl K8sClient {
    pub async fn new() -> Result<Self> {
        let config = Config::infer().await?;
        let namespace = config.default_namespace.clone();
        let client = Client::try_from(config)?;
        
        Ok(Self {
            client,
            current_namespace: namespace,
        })
    }
    
    pub fn client(&self) -> Client {
        self.client.clone()
    }
    
    pub fn current_namespace(&self) -> &str {
        &self.current_namespace
    }
    
    pub fn set_namespace(&mut self, namespace: String) {
        self.current_namespace = namespace;
    }
}
