use super::service::auth_session;
use super::{
    build_group_e2ee_add_rpc_params, build_group_e2ee_get_key_package_rpc_params, Client,
    MessageError, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::runtime::listener_service_did::message_service_did_from_capabilities_result;
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};

pub(crate) struct GroupE2eeTransport<'a> {
    resolved: &'a Resolved,
    client: Client,
    auth: crate::authsdk::Session,
    record: &'a StoredIdentity,
}

impl<'a> GroupE2eeTransport<'a> {
    pub(crate) fn new(
        resolved: &'a Resolved,
        manager: &Manager,
        record: &'a StoredIdentity,
    ) -> Result<Self, MessageError> {
        Ok(Self {
            resolved,
            client: Client::new(resolved)?,
            auth: auth_session(resolved, manager, record)?,
            record,
        })
    }

    pub(crate) fn rpc_call(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Map<String, Value>, MessageError> {
        self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            method,
            params,
            &mut self.auth,
        )
    }

    pub(crate) fn message_service_did(&mut self) -> Result<String, MessageError> {
        let configured = self.resolved.anp_service_did.trim();
        if !configured.is_empty() {
            return Ok(configured.to_string());
        }
        let result: Map<String, Value> = self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "anp.get_capabilities",
            json!({
                "meta": {
                    "anp_version": "1.0",
                    "profile": "anp.core.binding.v1",
                    "security_profile": "transport-protected",
                    "sender_did": self.record.did,
                    "operation_id": format!("op-{}", super::wire::generate_operation_id()),
                    "created_at": super::wire::now_rfc3339(),
                },
                "body": {},
                "client": {
                    "response_mode": "wait-final",
                },
            }),
            &mut self.auth,
        )?;
        message_service_did_from_capabilities_result(&result)
            .map_err(|err| MessageError::Internal(err.to_string()))
    }

    pub(crate) fn get_group_e2ee_key_package(
        &mut self,
        group_did: &str,
        member_did: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let service_did = self.message_service_did()?;
        let params = build_group_e2ee_get_key_package_rpc_params(
            self.record,
            &service_did,
            group_did,
            member_did,
        )?;
        self.rpc_call("group.e2ee.get_key_package", params)
    }

    pub(crate) fn add_group_e2ee(
        &mut self,
        group_did: &str,
        member_did: &str,
        mls_head: Map<String, Value>,
    ) -> Result<Map<String, Value>, MessageError> {
        let params = build_group_e2ee_add_rpc_params(self.record, group_did, member_did, mls_head)?;
        self.rpc_call("group.e2ee.add", params)
    }
}
