use super::*;

pub struct PreparedConfiguration {
    pub capabilities: Value,
    pub options: Value,
}

/// Initialize/query/configure only. No prompt, task token, or user tool server.
/// A newly allocated native session is deliberately not returned for storage.
pub async fn prepare(
    profile: CliRuntimeProfileRecord,
    cwd: PathBuf,
    existing_native: Option<String>,
    desired: Option<String>,
) -> Result<PreparedConfiguration> {
    let cwd = std::fs::canonicalize(cwd).context("acp_workspace_unavailable")?;
    let brand = Brand::parse(&profile.driver_id)?;
    let launch = launch_in_workspace(&profile, &cwd, existing_native.as_deref())?;
    let (launch, _hook) = configure_gemini_replay(brand, launch)?;
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let missing = Arc::new(AtomicBool::new(false));
    let missing_result = missing.clone();
    let startup_missing = missing.clone();
    let startup_native = existing_native.clone();
    let initialized = Arc::new(AtomicBool::new(false));
    let did_initialize = initialized.clone();
    let agent = AcpAgent::new(launch).with_debug(move |line, direction| {
        if brand == Brand::Gemini
            && direction == acp::LineDirection::Stderr
            && !initialized.load(Ordering::Acquire)
            && startup_native
                .as_deref()
                .is_some_and(|id| gemini_startup_missing(line, id))
        {
            startup_missing.store(true, Ordering::Release);
        }
    });
    let connection = acp::Client
        .builder()
        // Restoration may replay history; configuration never publishes it.
        .on_receive_notification(
            async |_notification: SessionNotification, _cx| Ok(()),
            acp::on_receive_notification!(),
        )
        .connect_with(agent, async move |cx: ConnectionTo<Agent>| {
            let initialized = cx.send_request(initialize_request()).block_task().await?;
            did_initialize.store(true, Ordering::Release);
            if initialized.protocol_version != ProtocolVersion::V1 {
                return Err(acp::Error::invalid_params());
            }
            let caps = serde_json::to_value(initialized.agent_capabilities).unwrap();
            let session =
                open_session(&cx, &caps, &cwd, existing_native.as_deref(), json!([])).await;
            let session = match session {
                Ok(value) => value,
                Err(error) => {
                    if let Some(id) = &existing_native {
                        let absent = missing_native_context(&error, brand, id)
                            || caps["loadSession"] != true
                                && !caps["sessionCapabilities"]["resume"].is_object()
                            || brand == Brand::OpenCode
                                && error.code == ErrorCode::InternalError
                                && error
                                    .data
                                    .as_ref()
                                    .is_some_and(|data| data["service"] == "session")
                                && caps["sessionCapabilities"]["list"].is_object()
                                && session_absent(&cx, &cwd, id).await;
                        missing_result.store(absent, Ordering::Release);
                    }
                    return Err(error);
                }
            };
            let native = existing_native
                .or_else(|| session["sessionId"].as_str().map(str::to_owned))
                .ok_or_else(acp::Error::invalid_params)?;
            let sid = SessionId::new(native);
            let mut options = session_options(&session);
            if let Some(desired) = desired {
                options = configure_model(&cx, &sid, options, &desired).await?;
            }
            *output.lock().unwrap() = Some(PreparedConfiguration {
                capabilities: caps.clone(),
                options,
            });
            if caps["sessionCapabilities"]["close"].is_object() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    cx.send_request(CloseSessionRequest::new(sid)).block_task(),
                )
                .await;
            }
            Ok(())
        });
    let done = tokio::time::timeout(Duration::from_secs(45), connection)
        .await
        .context("acp_configuration_timeout")?;
    if done.is_err() {
        bail!(if missing.load(Ordering::Acquire) {
            "context_reset_required"
        } else {
            "acp_configuration_failed"
        });
    }
    let prepared = result
        .lock()
        .unwrap()
        .take()
        .context("acp_configuration_failed");
    prepared
}

pub(super) async fn open_session(
    cx: &ConnectionTo<Agent>,
    caps: &Value,
    cwd: &std::path::Path,
    native: Option<&str>,
    servers: Value,
) -> Result<Value, acp::Error> {
    if let Some(native) = native {
        let params = json!({"sessionId":native,"cwd":cwd,"mcpServers":servers});
        if caps["sessionCapabilities"]["resume"].is_object() {
            let inner = serde_json::from_value(params).map_err(|_| acp::Error::invalid_params())?;
            cx.send_request(ResumeSessionWithModels { inner })
                .block_task()
                .await
        } else if caps["loadSession"] == true {
            let inner = serde_json::from_value(params).map_err(|_| acp::Error::invalid_params())?;
            cx.send_request(LoadSessionWithModels { inner })
                .block_task()
                .await
        } else {
            Err(acp::Error::invalid_params())
        }
    } else {
        let inner = serde_json::from_value(json!({"cwd":cwd,"mcpServers":servers}))
            .map_err(|_| acp::Error::invalid_params())?;
        cx.send_request(NewSessionWithModels { inner })
            .block_task()
            .await
    }
}

pub(super) async fn configure_model(
    cx: &ConnectionTo<Agent>,
    sid: &SessionId,
    mut options: Value,
    desired: &str,
) -> Result<Value, acp::Error> {
    if current_model(&options).as_deref() == Some(desired) {
        return Ok(options);
    }
    if !store::model_options(&options)
        .iter()
        .any(|model| model["id"] == desired)
    {
        return Err(acp::Error::invalid_params());
    }
    if let Some(option) = options.as_array().and_then(|items| {
        items
            .iter()
            .find(|v| v["category"] == "model" || v["id"] == "model")
    }) {
        let request: SetSessionConfigOptionRequest = serde_json::from_value(json!({
            "sessionId":sid,"configId":option["id"],"value":desired,
        }))
        .map_err(|_| acp::Error::invalid_params())?;
        let response = cx.send_request(request).block_task().await?;
        options = serde_json::to_value(response).map_err(|_| acp::Error::internal_error())?
            ["configOptions"]
            .clone();
        if current_model(&options).as_deref() != Some(desired) {
            return Err(acp::Error::invalid_params());
        }
    } else if options["availableModels"].is_array() {
        let response = cx
            .send_request(SetSessionModelRequest {
                session_id: sid.clone(),
                model_id: desired.to_owned(),
            })
            .block_task()
            .await?;
        // Legacy ACP set_model confirms success with an empty response. Only
        // after that acknowledgement may the requested value become effective.
        let returned = response
            .get("models")
            .cloned()
            .unwrap_or_else(|| options.clone());
        options = returned;
        if response.get("models").is_some() && current_model(&options).as_deref() != Some(desired) {
            return Err(acp::Error::invalid_params());
        }
        options["currentModelId"] = json!(desired);
    } else {
        return Err(acp::Error::invalid_params());
    }
    Ok(options)
}
