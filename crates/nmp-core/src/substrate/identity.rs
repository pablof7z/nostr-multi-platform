use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type IdentityId = String;

pub trait IdentityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Descriptor: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn scope_kind() -> IdentityScopeKind;
    fn create(
        ctx: &mut IdentityContext,
        descriptor: Self::Descriptor,
    ) -> Result<IdentityId, IdentityError>;
    fn sign<'a>(
        ctx: &'a IdentityContext,
        id: &'a IdentityId,
        unsigned: &'a UnsignedEvent,
    ) -> BoxFuture<'a, Result<SignedEvent, SigningError>>;
    fn destroy(ctx: &mut IdentityContext, id: &IdentityId);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IdentityScopeKind {
    HumanAccount,
    AppLocal,
    ExternalSigner,
    Ephemeral,
}

#[derive(Clone, Debug, Default)]
pub struct IdentityContext {
    created: Vec<IdentityId>,
}

impl IdentityContext {
    pub fn remember(&mut self, id: IdentityId) {
        self.created.push(id);
    }

    pub fn created(&self) -> &[IdentityId] {
        &self.created
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IdentityError {
    InvalidDescriptor(String),
    Storage(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnsignedEvent {
    pub pubkey: String,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedEvent {
    pub id: String,
    pub sig: String,
    pub unsigned: UnsignedEvent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SigningError {
    Unsupported(String),
    Rejected(String),
    Failed(String),
}
