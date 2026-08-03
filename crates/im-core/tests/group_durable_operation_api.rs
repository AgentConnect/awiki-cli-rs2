use awiki_im_core::groups::{
    GroupCreateRequest, GroupMemberMutationRequest, GroupReadResult, GroupService,
};
use awiki_im_core::ImResult;

#[allow(dead_code)]
fn compile_sync_api(
    service: &GroupService<'_>,
    create: GroupCreateRequest,
    add: GroupMemberMutationRequest,
) -> (ImResult<GroupReadResult>, ImResult<GroupReadResult>) {
    (
        service.create_with_operation_id(create, "durable-create-001"),
        service.add_member_with_operation_id(add, "durable-add-001"),
    )
}

#[allow(dead_code)]
async fn compile_async_api(
    service: &GroupService<'_>,
    create: GroupCreateRequest,
    add: GroupMemberMutationRequest,
) -> (ImResult<GroupReadResult>, ImResult<GroupReadResult>) {
    (
        service
            .create_with_operation_id_async(create, "durable-create-001")
            .await,
        service
            .add_member_with_operation_id_async(add, "durable-add-001")
            .await,
    )
}

#[test]
fn durable_group_operation_methods_are_public() {
    let _ = compile_sync_api;
    let _ = compile_async_api;
}
