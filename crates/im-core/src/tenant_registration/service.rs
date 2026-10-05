use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::internal::auth::session::{AsyncSessionProvider, FileSessionProvider, SessionProvider};
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport, CoreHttpTransport,
};
use crate::site::SiteDomain;

use super::dto::{invalid_response, validate_domain, validate_uuid};
use super::{
    AllowlistPage, ConfigureRegistration, ListRegistrationEntries, RegistrationOperation,
    RegistrationPolicy, RegistrationPolicySource,
};

const ENDPOINT: &str = "/user-service/v1/tenant-registration/rpc";

pub struct TenantRegistrationService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> TenantRegistrationService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    fn runtime(&self) -> Runtime<FileSessionProvider<'_>, CoreHttpTransport<'_>> {
        Runtime {
            session: FileSessionProvider::new(self.client),
            transport: CoreHttpTransport::new(self.client),
        }
    }

    pub fn get_policy(&self, domain: SiteDomain) -> crate::ImResult<RegistrationPolicy> {
        validate_domain(&domain)?;
        let value = self
            .runtime()
            .call("get_policy", json!({"domain": domain}))?;
        policy(value, &domain)
    }

    pub async fn get_policy_async(
        &self,
        domain: SiteDomain,
    ) -> crate::ImResult<RegistrationPolicy> {
        validate_domain(&domain)?;
        let value = self
            .runtime()
            .call_async("get_policy", json!({"domain": domain}))
            .await?;
        policy(value, &domain)
    }

    pub fn list_entries(&self, request: ListRegistrationEntries) -> crate::ImResult<AllowlistPage> {
        request.validate()?;
        let value = self.runtime().call("list_entries", encode(&request)?)?;
        page(value, &request)
    }

    pub async fn list_entries_async(
        &self,
        request: ListRegistrationEntries,
    ) -> crate::ImResult<AllowlistPage> {
        request.validate()?;
        let value = self
            .runtime()
            .call_async("list_entries", encode(&request)?)
            .await?;
        page(value, &request)
    }

    pub fn configure(
        &self,
        mut request: ConfigureRegistration,
    ) -> crate::ImResult<RegistrationOperation> {
        request.normalize()?;
        let value = self.runtime().call("configure", encode(&request)?)?;
        operation(value, &request.domain, &request.operation_id)
    }

    pub async fn configure_async(
        &self,
        mut request: ConfigureRegistration,
    ) -> crate::ImResult<RegistrationOperation> {
        request.normalize()?;
        let value = self
            .runtime()
            .call_async("configure", encode(&request)?)
            .await?;
        operation(value, &request.domain, &request.operation_id)
    }

    pub fn get_operation(
        &self,
        domain: SiteDomain,
        operation_id: &str,
    ) -> crate::ImResult<RegistrationOperation> {
        validate_domain(&domain)?;
        validate_uuid(operation_id)?;
        let value = self.runtime().call(
            "get_operation",
            json!({"domain": domain, "operation_id": operation_id}),
        )?;
        operation(value, &domain, operation_id)
    }

    pub async fn get_operation_async(
        &self,
        domain: SiteDomain,
        operation_id: &str,
    ) -> crate::ImResult<RegistrationOperation> {
        validate_domain(&domain)?;
        validate_uuid(operation_id)?;
        let value = self
            .runtime()
            .call_async(
                "get_operation",
                json!({"domain": domain, "operation_id": operation_id}),
            )
            .await?;
        operation(value, &domain, operation_id)
    }
}

pub(super) struct Runtime<P, T> {
    pub(super) session: P,
    pub(super) transport: T,
}

impl<P: SessionProvider, T: AuthenticatedRpcTransport> Runtime<P, T> {
    pub(super) fn call(&mut self, method: &str, params: Value) -> crate::ImResult<Value> {
        self.session
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        self.transport.authenticated_rpc(ENDPOINT, method, params)
    }
}

impl<P: AsyncSessionProvider, T: AsyncAuthenticatedRpcTransport> Runtime<P, T> {
    pub(super) async fn call_async(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.session
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        self.transport
            .authenticated_rpc(ENDPOINT, method, params)
            .await
    }
}

fn encode(value: &impl serde::Serialize) -> crate::ImResult<Value> {
    serde_json::to_value(value).map_err(|_| invalid_response())
}

fn decode<T: DeserializeOwned>(value: Value) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| invalid_response())
}

fn policy(value: Value, domain: &SiteDomain) -> crate::ImResult<RegistrationPolicy> {
    let policy: RegistrationPolicy = decode(value)?;
    policy.validate(domain)?;
    if policy.source != RegistrationPolicySource::Tenant {
        return Err(invalid_response());
    }
    Ok(policy)
}

fn page(value: Value, request: &ListRegistrationEntries) -> crate::ImResult<AllowlistPage> {
    let page: AllowlistPage = decode(value)?;
    page.validate(request)?;
    Ok(page)
}

fn operation(
    value: Value,
    domain: &SiteDomain,
    id: &str,
) -> crate::ImResult<RegistrationOperation> {
    let result: RegistrationOperation = decode(value)?;
    result.validate(domain, id)?;
    Ok(result)
}
